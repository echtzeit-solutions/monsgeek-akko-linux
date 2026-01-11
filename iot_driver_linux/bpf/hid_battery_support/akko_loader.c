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
 *   akko-loader --stop         Stop running loader (no sudo needed)
 *   akko-loader --status       Show loader status
 *
 * Options:
 *   -s, --strategy   Loading strategy: keyboard, vendor, wq (default: keyboard)
 *   -i, --hid-id     Override auto-detected HID ID
 *   -r, --refresh    F7 refresh interval in seconds (default: 600, vendor only)
 *   -d, --daemon     Run as daemon (fork to background)
 *   -v, --verbose    Verbose output
 *   --stop           Stop running loader (no sudo required)
 *   --status         Show loader status
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

#define VERSION "1.1.0"

/* PID and stop file paths - in /tmp so any user can access */
#define PID_FILE "/tmp/akko-loader.pid"
#define STOP_FILE "/tmp/akko-loader.stop"

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
    .refresh_interval = 600,
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

/* Write PID file */
static void write_pid_file(void)
{
    FILE *f = fopen(PID_FILE, "w");
    if (f) {
        fprintf(f, "%d\n", getpid());
        fclose(f);
        /* Make world-readable so --status works without sudo */
        chmod(PID_FILE, 0644);
    }
}

/* Remove PID and stop files on exit */
static void cleanup_files(void)
{
    unlink(PID_FILE);
    unlink(STOP_FILE);
}

/* Check if stop file exists (non-root stop mechanism) */
static int check_stop_file(void)
{
    return access(STOP_FILE, F_OK) == 0;
}

/* Read PID from file, returns -1 if not found */
static pid_t read_pid_file(void)
{
    FILE *f = fopen(PID_FILE, "r");
    if (!f)
        return -1;

    pid_t pid = -1;
    if (fscanf(f, "%d", &pid) != 1)
        pid = -1;
    fclose(f);
    return pid;
}

/* Check if process is running */
static int process_running(pid_t pid)
{
    if (pid <= 0)
        return 0;
    return kill(pid, 0) == 0;
}

/* Stop command - create stop file to signal loader */
static int do_stop(void)
{
    pid_t pid = read_pid_file();

    if (pid <= 0) {
        fprintf(stderr, "No loader running (PID file not found)\n");
        return 1;
    }

    if (!process_running(pid)) {
        fprintf(stderr, "Loader not running (stale PID file)\n");
        unlink(PID_FILE);
        return 1;
    }

    /* Create stop file - loader will see this and exit */
    FILE *f = fopen(STOP_FILE, "w");
    if (!f) {
        fprintf(stderr, "Failed to create stop file: %s\n", strerror(errno));
        return 1;
    }
    fclose(f);

    fprintf(stderr, "Signaling loader (PID %d) to stop...\n", pid);

    /* Wait for loader to exit (up to 5 seconds) */
    for (int i = 0; i < 50; i++) {
        usleep(100000);
        if (!process_running(pid)) {
            fprintf(stderr, "Loader stopped\n");
            unlink(STOP_FILE);
            return 0;
        }
    }

    fprintf(stderr, "Loader did not stop in time, sending SIGTERM...\n");
    kill(pid, SIGTERM);
    unlink(STOP_FILE);
    return 0;
}

/* Status command - show loader state */
static int do_status(void)
{
    pid_t pid = read_pid_file();

    printf("Akko Loader Status:\n");

    if (pid <= 0) {
        printf("  Status: not running (no PID file)\n");
        return 1;
    }

    if (!process_running(pid)) {
        printf("  Status: not running (stale PID file, PID was %d)\n", pid);
        return 1;
    }

    printf("  Status: running\n");
    printf("  PID: %d\n", pid);

    /* Show battery if available */
    DIR *dir = opendir("/sys/class/power_supply");
    if (dir) {
        struct dirent *ent;
        while ((ent = readdir(dir)) != NULL) {
            if (strstr(ent->d_name, "3151")) {
                char path[256];
                snprintf(path, sizeof(path), "/sys/class/power_supply/%s/capacity", ent->d_name);
                FILE *f = fopen(path, "r");
                if (f) {
                    int cap;
                    if (fscanf(f, "%d", &cap) == 1) {
                        printf("  Battery: %d%%\n", cap);
                    }
                    fclose(f);
                }
            }
        }
        closedir(dir);
    }

    return 0;
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
    fprintf(stderr, "  -r, --refresh <sec>     F7 refresh interval (default: 600 = 10min)\n");
    fprintf(stderr, "  -d, --daemon            Run as daemon (fork to background)\n");
    fprintf(stderr, "  -v, --verbose           Verbose output\n");
    fprintf(stderr, "  --stop                  Stop running loader (no sudo needed)\n");
    fprintf(stderr, "  --status                Show loader status (no sudo needed)\n");
    fprintf(stderr, "  -h, --help              Show this help\n");
    fprintf(stderr, "\nExamples:\n");
    fprintf(stderr, "  %s                      # Use keyboard strategy (default)\n", prog);
    fprintf(stderr, "  %s -s vendor -d         # Vendor strategy as daemon\n", prog);
    fprintf(stderr, "  %s -s wq                # Self-contained bpf_wq strategy\n", prog);
    fprintf(stderr, "  %s --stop               # Stop running loader\n", prog);
    fprintf(stderr, "  %s --status             # Check if loader is running\n", prog);
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
        fprintf(stderr, "Stop with: akko-loader --stop (or Ctrl+C)\n");
    } else if (config.strategy == STRATEGY_VENDOR) {
        fprintf(stderr, "\nF7 refresh every %d seconds.\n", config.refresh_interval);
        fprintf(stderr, "Stop with: akko-loader --stop (or Ctrl+C)\n");
    } else {
        fprintf(stderr, "\nKeyboard strategy - no refresh needed.\n");
        fprintf(stderr, "Stop with: akko-loader --stop (or Ctrl+C)\n");
    }

    while (running) {
        sleep(1);
        seconds_since_f7++;

        /* Check for stop file (allows non-root to stop loader) */
        if (check_stop_file()) {
            if (config.verbose)
                fprintf(stderr, "Stop file detected, exiting...\n");
            running = 0;
            break;
        }

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
        {"stop", no_argument, 0, 'S'},
        {"status", no_argument, 0, 'T'},
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
        case 'S':  /* --stop */
            return do_stop();
        case 'T':  /* --status */
            return do_status();
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

    /* Write PID file for --stop/--status commands */
    write_pid_file();

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
    cleanup_files();
    cleanup_bpf();
    fprintf(stderr, "Done\n");

    return 0;
}
