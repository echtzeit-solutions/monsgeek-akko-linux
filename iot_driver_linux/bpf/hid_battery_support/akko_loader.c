// SPDX-License-Identifier: GPL-2.0
/*
 * Unified HID-BPF loader for Akko/MonsGeek keyboard battery integration
 *
 * Supports multiple loading strategies:
 *   -s keyboard    Option A: Inject battery into keyboard interface (00D8)
 *   -s vendor      Option B: Use vendor interface with loader F7 refresh
 *   -s wq          Option B WQ: Use vendor interface with bpf_wq auto-refresh
 *
 * Usage:
 *   akko-loader -s <strategy> [-i <hid_id>] [-r <refresh_sec>] [-d]
 *
 * Options:
 *   -s, --strategy   Loading strategy: keyboard, vendor, wq (default: keyboard)
 *   -i, --hid-id     Override auto-detected HID ID
 *   -r, --refresh    F7 refresh interval in seconds (default: 30, vendor only)
 *   -d, --daemon     Run as daemon (fork to background)
 *   -v, --verbose    Verbose output
 *   -h, --help       Show help
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <getopt.h>
#include <dirent.h>
#include <signal.h>
#include <fcntl.h>
#include <sys/ioctl.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <linux/hidraw.h>
#include <bpf/libbpf.h>

/* Include all strategy skeletons */
#include "option_a_keyboard_inject/akko_keyboard_battery.skel.h"
#include "option_b_bidirectional/akko_bidirectional.skel.h"
#include "option_b_wq_experimental/akko_wq.skel.h"

#define VID 0x3151
#define PID 0x5038

#define VERSION "1.0.0"

/* Strategy types */
typedef enum {
    STRATEGY_KEYBOARD,  /* Option A: keyboard interface */
    STRATEGY_VENDOR,    /* Option B: vendor interface with loader refresh */
    STRATEGY_WQ,        /* Option B WQ: vendor interface with bpf_wq */
} strategy_t;

/* Configuration */
static struct {
    strategy_t strategy;
    int hid_id;
    int refresh_interval;
    int daemon_mode;
    int verbose;
} config = {
    .strategy = STRATEGY_KEYBOARD,
    .hid_id = -1,
    .refresh_interval = 30,
    .daemon_mode = 0,
    .verbose = 0,
};

/* Runtime state */
static volatile int running = 1;
static char g_device_name[64];
static char g_hidraw_path[64];

/* BPF skeleton pointers (only one will be used) */
static struct akko_keyboard_battery_bpf *skel_keyboard = NULL;
static struct akko_bidirectional_bpf *skel_vendor = NULL;
static struct akko_wq_bpf *skel_wq = NULL;

static void sig_handler(int sig)
{
    (void)sig;
    running = 0;
}

static void print_usage(const char *prog)
{
    fprintf(stderr, "Akko/MonsGeek Keyboard Battery BPF Loader v%s\n\n", VERSION);
    fprintf(stderr, "Usage: %s [options]\n\n", prog);
    fprintf(stderr, "Options:\n");
    fprintf(stderr, "  -s, --strategy <name>   Loading strategy (default: keyboard)\n");
    fprintf(stderr, "                          keyboard - Inject into keyboard interface (recommended)\n");
    fprintf(stderr, "                          vendor   - Use vendor interface, loader F7 refresh\n");
    fprintf(stderr, "                          wq       - Use vendor interface, bpf_wq auto-refresh\n");
    fprintf(stderr, "  -i, --hid-id <id>       Override auto-detected HID ID\n");
    fprintf(stderr, "  -r, --refresh <sec>     F7 refresh interval (default: 30, vendor strategy only)\n");
    fprintf(stderr, "  -d, --daemon            Run as daemon (fork to background)\n");
    fprintf(stderr, "  -v, --verbose           Verbose output\n");
    fprintf(stderr, "  -h, --help              Show this help\n");
    fprintf(stderr, "\nExamples:\n");
    fprintf(stderr, "  %s                      # Use keyboard strategy (default)\n", prog);
    fprintf(stderr, "  %s -s vendor -d         # Vendor strategy as daemon\n", prog);
    fprintf(stderr, "  %s -s wq                # Self-contained bpf_wq strategy\n", prog);
}

static int parse_strategy(const char *name)
{
    if (strcmp(name, "keyboard") == 0 || strcmp(name, "kb") == 0 || strcmp(name, "a") == 0)
        return STRATEGY_KEYBOARD;
    if (strcmp(name, "vendor") == 0 || strcmp(name, "b") == 0)
        return STRATEGY_VENDOR;
    if (strcmp(name, "wq") == 0 || strcmp(name, "workqueue") == 0)
        return STRATEGY_WQ;
    return -1;
}

static const char *strategy_name(strategy_t s)
{
    switch (s) {
        case STRATEGY_KEYBOARD: return "keyboard";
        case STRATEGY_VENDOR: return "vendor";
        case STRATEGY_WQ: return "wq";
        default: return "unknown";
    }
}

/* Kill any other running loader instances */
static void kill_previous_loaders(void)
{
    DIR *dir;
    struct dirent *ent;
    pid_t my_pid = getpid();
    char path[256], cmdline[512];
    FILE *f;

    dir = opendir("/proc");
    if (!dir)
        return;

    while ((ent = readdir(dir)) != NULL) {
        pid_t pid = atoi(ent->d_name);
        if (pid <= 0 || pid == my_pid)
            continue;

        snprintf(path, sizeof(path), "/proc/%d/cmdline", pid);
        f = fopen(path, "r");
        if (!f)
            continue;

        size_t len = fread(cmdline, 1, sizeof(cmdline) - 1, f);
        fclose(f);
        cmdline[len] = '\0';

        if (strstr(cmdline, "akko") && strstr(cmdline, "loader")) {
            if (config.verbose)
                fprintf(stderr, "Killing previous loader (PID %d)...\n", pid);
            kill(pid, SIGTERM);
        }
    }

    closedir(dir);
    usleep(300000);
}

/* Find hidraw device for HID interface */
static char *find_hidraw_for_hid(const char *hid_name)
{
    static char hidraw_path[64];
    char path[512];
    DIR *dir;
    struct dirent *ent;

    snprintf(path, sizeof(path), "/sys/bus/hid/devices/%s/hidraw", hid_name);
    dir = opendir(path);
    if (!dir)
        return NULL;

    while ((ent = readdir(dir)) != NULL) {
        if (strncmp(ent->d_name, "hidraw", 6) == 0) {
            snprintf(hidraw_path, sizeof(hidraw_path), "/dev/%s", ent->d_name);
            closedir(dir);
            return hidraw_path;
        }
    }

    closedir(dir);
    return NULL;
}

/* Send F7 command to refresh battery */
static int send_f7_command(const char *hidraw_path)
{
    int fd;
    unsigned char buf[65] = {0};
    int ret;

    if (config.verbose)
        fprintf(stderr, "Sending F7 command to prime battery cache...\n");

    fd = open(hidraw_path, O_RDWR);
    if (fd < 0) {
        if (config.verbose)
            fprintf(stderr, "  Failed to open hidraw: %s\n", strerror(errno));
        return -1;
    }

    buf[0] = 0x00;
    buf[1] = 0xF7;

    ret = ioctl(fd, HIDIOCSFEATURE(65), buf);
    if (ret < 0) {
        if (config.verbose)
            fprintf(stderr, "  SET_FEATURE failed: %s\n", strerror(errno));
        close(fd);
        return -1;
    }

    usleep(100000);

    buf[0] = 0x00;
    ret = ioctl(fd, HIDIOCGFEATURE(65), buf);
    if (ret >= 0 && buf[1] > 0 && buf[1] <= 100) {
        if (config.verbose)
            fprintf(stderr, "  Battery: %d%%\n", buf[1]);
    }

    close(fd);
    return 0;
}

/* Rebind HID device */
static void rebind_hid_device(const char *device_name)
{
    char path[256];
    FILE *f;

    if (config.verbose)
        fprintf(stderr, "Rebinding device %s...\n", device_name);

    snprintf(path, sizeof(path), "/sys/bus/hid/drivers/hid-generic/unbind");
    f = fopen(path, "w");
    if (f) {
        fprintf(f, "%s", device_name);
        fclose(f);
    }

    usleep(100000);

    snprintf(path, sizeof(path), "/sys/bus/hid/drivers/hid-generic/bind");
    f = fopen(path, "w");
    if (f) {
        fprintf(f, "%s", device_name);
        fclose(f);
    }

    usleep(100000);
    if (config.verbose)
        fprintf(stderr, "Device rebound\n");
}

/* Find HID interface based on strategy */
static int find_hid_interface(void)
{
    DIR *dir;
    struct dirent *ent;
    char path[512];
    unsigned char rdesc[64];
    FILE *f;
    int hid_id = -1;

    /* Target descriptor pattern based on strategy */
    int want_vendor = (config.strategy == STRATEGY_VENDOR || config.strategy == STRATEGY_WQ);

    if (config.verbose) {
        fprintf(stderr, "Searching for %s interface VID=%04x PID=%04x...\n",
                want_vendor ? "vendor" : "keyboard", VID, PID);
    }

    dir = opendir("/sys/bus/hid/devices");
    if (!dir) {
        perror("opendir /sys/bus/hid/devices");
        return -1;
    }

    while ((ent = readdir(dir)) != NULL) {
        unsigned int bus, vid, pid, id;

        if (sscanf(ent->d_name, "%x:%x:%x.%x", &bus, &vid, &pid, &id) != 4)
            continue;

        if (vid != VID || pid != PID)
            continue;

        if (config.verbose)
            fprintf(stderr, "  Checking %s...\n", ent->d_name);

        snprintf(path, sizeof(path), "/sys/bus/hid/devices/%s/report_descriptor",
                 ent->d_name);
        f = fopen(path, "rb");
        if (!f)
            continue;

        size_t len = fread(rdesc, 1, sizeof(rdesc), f);
        fclose(f);

        if (config.verbose) {
            fprintf(stderr, "    Descriptor size=%zu, first bytes: %02x %02x %02x\n",
                    len, rdesc[0], rdesc[1], rdesc[2]);
        }

        int is_vendor = (len >= 3 && rdesc[0] == 0x06 && rdesc[1] == 0xFF && rdesc[2] == 0xFF);
        int is_keyboard = (len >= 3 && rdesc[0] == 0x05 && rdesc[1] == 0x01 && rdesc[2] == 0x09);

        if ((want_vendor && is_vendor) || (!want_vendor && is_keyboard)) {
            fprintf(stderr, "Found %s interface: %s (hid_id=%u)\n",
                    want_vendor ? "vendor" : "keyboard", ent->d_name, id);
            strncpy(g_device_name, ent->d_name, sizeof(g_device_name) - 1);
            hid_id = id;

            /* For vendor strategies, get hidraw path and send F7 */
            if (want_vendor) {
                char *hidraw = find_hidraw_for_hid(ent->d_name);
                if (hidraw) {
                    strncpy(g_hidraw_path, hidraw, sizeof(g_hidraw_path) - 1);
                    send_f7_command(hidraw);
                }
            }
            break;
        }
    }

    closedir(dir);
    return hid_id;
}

/* Load and attach BPF based on strategy */
static int load_bpf(int hid_id)
{
    int err;

    fprintf(stderr, "Loading BPF strategy: %s\n", strategy_name(config.strategy));

    switch (config.strategy) {
    case STRATEGY_KEYBOARD:
        skel_keyboard = akko_keyboard_battery_bpf__open();
        if (!skel_keyboard) {
            fprintf(stderr, "Failed to open keyboard skeleton: %s\n", strerror(errno));
            return -1;
        }
        skel_keyboard->struct_ops.akko_keyboard_battery->hid_id = hid_id;
        err = akko_keyboard_battery_bpf__load(skel_keyboard);
        if (err) {
            fprintf(stderr, "Failed to load keyboard BPF: %s\n", strerror(-err));
            akko_keyboard_battery_bpf__destroy(skel_keyboard);
            return -1;
        }
        err = akko_keyboard_battery_bpf__attach(skel_keyboard);
        if (err) {
            fprintf(stderr, "Failed to attach keyboard BPF: %s\n", strerror(-err));
            akko_keyboard_battery_bpf__destroy(skel_keyboard);
            return -1;
        }
        break;

    case STRATEGY_VENDOR:
        skel_vendor = akko_bidirectional_bpf__open();
        if (!skel_vendor) {
            fprintf(stderr, "Failed to open vendor skeleton: %s\n", strerror(errno));
            return -1;
        }
        skel_vendor->struct_ops.akko_bidirectional->hid_id = hid_id;
        err = akko_bidirectional_bpf__load(skel_vendor);
        if (err) {
            fprintf(stderr, "Failed to load vendor BPF: %s\n", strerror(-err));
            akko_bidirectional_bpf__destroy(skel_vendor);
            return -1;
        }
        err = akko_bidirectional_bpf__attach(skel_vendor);
        if (err) {
            fprintf(stderr, "Failed to attach vendor BPF: %s\n", strerror(-err));
            akko_bidirectional_bpf__destroy(skel_vendor);
            return -1;
        }
        break;

    case STRATEGY_WQ:
        skel_wq = akko_wq_bpf__open();
        if (!skel_wq) {
            fprintf(stderr, "Failed to open wq skeleton: %s\n", strerror(errno));
            return -1;
        }
        skel_wq->struct_ops.akko_wq->hid_id = hid_id;
        err = akko_wq_bpf__load(skel_wq);
        if (err) {
            fprintf(stderr, "Failed to load wq BPF: %s\n", strerror(-err));
            akko_wq_bpf__destroy(skel_wq);
            return -1;
        }
        err = akko_wq_bpf__attach(skel_wq);
        if (err) {
            fprintf(stderr, "Failed to attach wq BPF: %s\n", strerror(-err));
            akko_wq_bpf__destroy(skel_wq);
            return -1;
        }
        break;
    }

    fprintf(stderr, "BPF loaded and attached successfully!\n");
    return 0;
}

/* Cleanup BPF */
static void cleanup_bpf(void)
{
    if (skel_keyboard) {
        akko_keyboard_battery_bpf__destroy(skel_keyboard);
        skel_keyboard = NULL;
    }
    if (skel_vendor) {
        akko_bidirectional_bpf__destroy(skel_vendor);
        skel_vendor = NULL;
    }
    if (skel_wq) {
        akko_wq_bpf__destroy(skel_wq);
        skel_wq = NULL;
    }
}

/* Show power supplies */
static void show_power_supplies(void)
{
    DIR *dir = opendir("/sys/class/power_supply");
    if (!dir)
        return;

    fprintf(stderr, "\n=== Power supplies ===\n");
    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL) {
        if (ent->d_name[0] != '.')
            fprintf(stderr, "%s\n", ent->d_name);
    }
    closedir(dir);
}

/* Main loop based on strategy */
static void run_loop(void)
{
    int seconds_since_f7 = 0;

    /* WQ strategy doesn't need refresh loop - loader can exit */
    if (config.strategy == STRATEGY_WQ) {
        fprintf(stderr, "\nbpf_wq handles F7 refresh automatically.\n");
        fprintf(stderr, "Press Ctrl+C to unload, or loader can exit safely.\n");
    } else if (config.strategy == STRATEGY_VENDOR) {
        fprintf(stderr, "\nF7 refresh every %d seconds.\n", config.refresh_interval);
        fprintf(stderr, "Press Ctrl+C to unload...\n");
    } else {
        fprintf(stderr, "\nKeyboard strategy - no refresh needed.\n");
        fprintf(stderr, "Press Ctrl+C to unload...\n");
    }

    while (running) {
        sleep(1);
        seconds_since_f7++;

        /* Only vendor strategy needs periodic F7 from loader */
        if (config.strategy == STRATEGY_VENDOR &&
            seconds_since_f7 >= config.refresh_interval && g_hidraw_path[0]) {
            int fd = open(g_hidraw_path, O_RDWR);
            if (fd >= 0) {
                unsigned char buf[65] = {0};
                buf[0] = 0x00;
                buf[1] = 0xF7;
                ioctl(fd, HIDIOCSFEATURE(65), buf);
                close(fd);
                if (config.verbose)
                    fprintf(stderr, "F7 refresh sent\n");
            }
            seconds_since_f7 = 0;
        }
    }
}

int main(int argc, char **argv)
{
    int hid_id;
    int opt;

    static struct option long_options[] = {
        {"strategy", required_argument, 0, 's'},
        {"hid-id", required_argument, 0, 'i'},
        {"refresh", required_argument, 0, 'r'},
        {"daemon", no_argument, 0, 'd'},
        {"verbose", no_argument, 0, 'v'},
        {"help", no_argument, 0, 'h'},
        {0, 0, 0, 0}
    };

    while ((opt = getopt_long(argc, argv, "s:i:r:dvh", long_options, NULL)) != -1) {
        switch (opt) {
        case 's':
            config.strategy = parse_strategy(optarg);
            if (config.strategy < 0) {
                fprintf(stderr, "Unknown strategy: %s\n", optarg);
                print_usage(argv[0]);
                return 1;
            }
            break;
        case 'i':
            config.hid_id = atoi(optarg);
            break;
        case 'r':
            config.refresh_interval = atoi(optarg);
            if (config.refresh_interval < 5) {
                fprintf(stderr, "Refresh interval must be >= 5 seconds\n");
                return 1;
            }
            break;
        case 'd':
            config.daemon_mode = 1;
            break;
        case 'v':
            config.verbose = 1;
            break;
        case 'h':
            print_usage(argv[0]);
            return 0;
        default:
            print_usage(argv[0]);
            return 1;
        }
    }

    if (geteuid() != 0) {
        fprintf(stderr, "Error: Must run as root\n");
        return 1;
    }

    fprintf(stderr, "Akko/MonsGeek Keyboard Battery Loader v%s\n", VERSION);
    fprintf(stderr, "Strategy: %s\n\n", strategy_name(config.strategy));

    /* Kill previous loaders */
    kill_previous_loaders();

    /* Find HID interface or use provided ID */
    if (config.hid_id > 0) {
        hid_id = config.hid_id;
        fprintf(stderr, "Using provided hid_id=%d\n", hid_id);
    } else {
        hid_id = find_hid_interface();
        if (hid_id < 0) {
            fprintf(stderr, "Could not find suitable HID interface\n");
            fprintf(stderr, "Make sure the dongle is connected\n");
            return 1;
        }
    }

    /* Load BPF */
    if (load_bpf(hid_id) != 0) {
        return 1;
    }

    /* Rebind device */
    if (g_device_name[0]) {
        rebind_hid_device(g_device_name);
        usleep(500000);
    }

    show_power_supplies();

    /* Daemonize if requested */
    if (config.daemon_mode) {
        pid_t pid = fork();
        if (pid < 0) {
            perror("fork");
            cleanup_bpf();
            return 1;
        }
        if (pid > 0) {
            /* Parent exits */
            fprintf(stderr, "Daemonized with PID %d\n", pid);
            return 0;
        }
        /* Child continues */
        setsid();
        close(STDIN_FILENO);
        close(STDOUT_FILENO);
        close(STDERR_FILENO);
    }

    /* Setup signal handlers */
    signal(SIGINT, sig_handler);
    signal(SIGTERM, sig_handler);

    /* Run main loop */
    run_loop();

    fprintf(stderr, "\nUnloading BPF program...\n");
    cleanup_bpf();
    fprintf(stderr, "Done\n");

    return 0;
}
