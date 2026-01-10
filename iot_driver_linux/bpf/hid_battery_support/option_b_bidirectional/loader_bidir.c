// SPDX-License-Identifier: GPL-2.0
/*
 * HID-BPF loader for Akko keyboard battery integration
 * Option B: Vendor interface with periodic F7 refresh
 *
 * Loads BPF that attaches to vendor interface (00DA). The loader
 * sends periodic F7 commands to refresh battery data from the keyboard.
 *
 * Note: bpf_wq approach caused kernel verifier crash on 6.17, so we
 * use loader-based F7 refresh instead.
 *
 * The loader:
 * 1. Finds the vendor interface
 * 2. Sends initial F7 to prime battery cache
 * 3. Loads and attaches BPF program
 * 4. Rebinds device to apply new descriptor
 * 5. Periodically sends F7 to keep battery fresh
 *
 * Usage: sudo ./loader_bidir [hid_id]
 *   If hid_id not specified, auto-detects vendor interface
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <dirent.h>
#include <signal.h>
#include <fcntl.h>
#include <sys/ioctl.h>
#include <sys/types.h>
#include <linux/hidraw.h>

#include "akko_bidirectional.skel.h"

#define VID 0x3151
#define PID 0x5038

static volatile int running = 1;

static void sig_handler(int sig)
{
    (void)sig;
    running = 0;
}

/* Kill any other running loader processes */
static void kill_previous_loaders(void)
{
    DIR *dir;
    struct dirent *ent;
    pid_t my_pid = getpid();
    char path[256], cmdline[256];
    FILE *f;
    int killed = 0;

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

        /* Check if this is another loader instance */
        if (strstr(cmdline, "loader")) {
            fprintf(stderr, "Killing previous loader (PID %d)...\n", pid);
            kill(pid, SIGTERM);
            killed++;
        }
    }

    closedir(dir);

    if (killed > 0) {
        usleep(500000);
    }
}

/* Find hidraw device for vendor interface */
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

/* Send F7 command to refresh battery via hidraw */
static int send_f7_command(const char *hidraw_path)
{
    int fd;
    unsigned char buf[65] = {0};
    int ret;

    /* Do not print full device path to avoid Claude reading from it */
    fprintf(stderr, "Sending initial F7 command to prime battery cache...\n");

    fd = open(hidraw_path, O_RDWR);
    if (fd < 0) {
        fprintf(stderr, "  Failed to open hidraw: %s\n", strerror(errno));
        return -1;
    }

    /* F7 command: Report ID 0, command F7 */
    buf[0] = 0x00;  /* Report ID */
    buf[1] = 0xF7;  /* F7 command */

    /* Send SET_FEATURE */
    ret = ioctl(fd, HIDIOCSFEATURE(65), buf);
    if (ret < 0) {
        fprintf(stderr, "  SET_FEATURE failed: %s\n", strerror(errno));
        close(fd);
        return -1;
    }

    /* Wait for RF round-trip */
    usleep(100000);

    /* Read battery to verify */
    buf[0] = 0x00;  /* Report ID */
    ret = ioctl(fd, HIDIOCGFEATURE(65), buf);
    if (ret < 0) {
        fprintf(stderr, "  GET_FEATURE failed: %s\n", strerror(errno));
        close(fd);
        return -1;
    }

    if (buf[1] > 0 && buf[1] <= 100) {
        fprintf(stderr, "  Battery read successful: %d%%\n", buf[1]);
    } else {
        fprintf(stderr, "  Battery data not available yet (will retry via BPF)\n");
    }

    close(fd);
    return 0;
}

/* Rebind HID device to trigger descriptor re-parse */
static void rebind_hid_device(const char *device_name)
{
    char path[256];
    FILE *f;

    fprintf(stderr, "Rebinding device %s...\n", device_name);

    /* Unbind from hid-generic */
    snprintf(path, sizeof(path), "/sys/bus/hid/drivers/hid-generic/unbind");
    f = fopen(path, "w");
    if (f) {
        fprintf(f, "%s", device_name);
        fclose(f);
    }

    usleep(100000);

    /* Rebind to hid-generic */
    snprintf(path, sizeof(path), "/sys/bus/hid/drivers/hid-generic/bind");
    f = fopen(path, "w");
    if (f) {
        fprintf(f, "%s", device_name);
        fclose(f);
    }

    usleep(100000);
    fprintf(stderr, "Device rebound\n");
}

/* Global to store device name for rebind */
static char g_device_name[64];
static char g_hidraw_path[64];

/* F7 refresh interval in seconds */
#define F7_REFRESH_INTERVAL 30

/* Find vendor HID interface (descriptor starts with 06 FF FF) */
static int find_vendor_interface(void)
{
    DIR *dir;
    struct dirent *ent;
    char path[512];
    unsigned char rdesc[64];
    FILE *f;
    int hid_id = -1;

    fprintf(stderr, "Searching for vendor interface VID=%04x PID=%04x...\n", VID, PID);

    dir = opendir("/sys/bus/hid/devices");
    if (!dir) {
        perror("opendir /sys/bus/hid/devices");
        return -1;
    }

    while ((ent = readdir(dir)) != NULL) {
        unsigned int bus, vid, pid, id;

        /* Parse device name: BBBB:VVVV:PPPP.IIII */
        if (sscanf(ent->d_name, "%x:%x:%x.%x", &bus, &vid, &pid, &id) != 4)
            continue;

        if (vid != VID || pid != PID)
            continue;

        fprintf(stderr, "  Checking %s...\n", ent->d_name);

        /* Check descriptor - vendor interface starts with 06 FF FF */
        snprintf(path, sizeof(path), "/sys/bus/hid/devices/%s/report_descriptor",
                 ent->d_name);
        f = fopen(path, "rb");
        if (!f)
            continue;

        size_t len = fread(rdesc, 1, sizeof(rdesc), f);
        fclose(f);

        fprintf(stderr, "    Descriptor size=%zu, first bytes: %02x %02x %02x\n",
                len, rdesc[0], rdesc[1], rdesc[2]);

        /* Check for vendor interface: 06 FF FF (Usage Page 0xFFFF) */
        if (len >= 3 &&
            rdesc[0] == 0x06 && rdesc[1] == 0xFF && rdesc[2] == 0xFF) {
            fprintf(stderr, "Found vendor interface: %s (hid_id=%u)\n", ent->d_name, id);
            strncpy(g_device_name, ent->d_name, sizeof(g_device_name) - 1);
            hid_id = id;

            /* Store hidraw path for periodic refresh */
            char *hidraw = find_hidraw_for_hid(ent->d_name);
            if (hidraw) {
                strncpy(g_hidraw_path, hidraw, sizeof(g_hidraw_path) - 1);
                send_f7_command(hidraw);
            }
            break;
        }
    }

    closedir(dir);
    return hid_id;
}

int main(int argc, char **argv)
{
    struct akko_bidirectional_bpf *skel;
    int hid_id;
    int err;

    fprintf(stderr, "Akko Keyboard Battery BPF loader (Option B - Vendor Interface)\n");
    fprintf(stderr, "Periodic F7 refresh to keep battery data fresh\n\n");

    if (geteuid() != 0) {
        fprintf(stderr, "Error: Must run as root\n");
        return 1;
    }

    /* Kill any previous loader instances */
    kill_previous_loaders();

    /* Get hid_id from argument or auto-detect */
    if (argc > 1) {
        hid_id = atoi(argv[1]);
        if (hid_id <= 0) {
            fprintf(stderr, "Invalid hid_id: %s\n", argv[1]);
            return 1;
        }
        fprintf(stderr, "Using provided hid_id=%d\n", hid_id);
    } else {
        hid_id = find_vendor_interface();
        if (hid_id < 0) {
            fprintf(stderr, "Could not find vendor interface\n");
            fprintf(stderr, "Make sure the dongle is connected\n");
            return 1;
        }
    }

    /* Open BPF skeleton */
    fprintf(stderr, "Opening BPF skeleton...\n");
    skel = akko_bidirectional_bpf__open();
    if (!skel) {
        fprintf(stderr, "Failed to open BPF skeleton: %s\n", strerror(errno));
        return 1;
    }

    /* Set hid_id BEFORE loading */
    fprintf(stderr, "Setting hid_id=%d in struct_ops...\n", hid_id);
    skel->struct_ops.akko_bidirectional->hid_id = hid_id;

    /* Load BPF programs */
    fprintf(stderr, "Loading BPF programs...\n");
    err = akko_bidirectional_bpf__load(skel);
    if (err) {
        fprintf(stderr, "Failed to load BPF: %s\n", strerror(-err));
        akko_bidirectional_bpf__destroy(skel);
        return 1;
    }
    fprintf(stderr, "BPF loaded successfully\n");

    /* Attach struct_ops */
    fprintf(stderr, "Attaching struct_ops...\n");
    err = akko_bidirectional_bpf__attach(skel);
    if (err) {
        fprintf(stderr, "Failed to attach BPF: %s\n", strerror(-err));
        akko_bidirectional_bpf__destroy(skel);
        return 1;
    }

    fprintf(stderr, "BPF program loaded and attached successfully!\n");

    /* Rebind the device to trigger descriptor re-parse */
    if (g_device_name[0]) {
        rebind_hid_device(g_device_name);

        /* Check if power_supply was created */
        usleep(500000);
        DIR *dir = opendir("/sys/class/power_supply");
        if (dir) {
            struct dirent *ent;
            fprintf(stderr, "\n=== Power supplies ===\n");
            while ((ent = readdir(dir)) != NULL) {
                if (ent->d_name[0] != '.')
                    fprintf(stderr, "%s\n", ent->d_name);
            }
            closedir(dir);
        }
    }

    fprintf(stderr, "\nPress Ctrl+C to unload...\n");
    fprintf(stderr, "F7 refresh commands will be sent every %d seconds\n", F7_REFRESH_INTERVAL);

    /* Handle signals for clean shutdown */
    signal(SIGINT, sig_handler);
    signal(SIGTERM, sig_handler);

    /* Keep running and periodically refresh battery */
    int seconds_since_f7 = 0;
    while (running) {
        sleep(1);
        seconds_since_f7++;

        if (seconds_since_f7 >= F7_REFRESH_INTERVAL && g_hidraw_path[0]) {
            /* Send F7 to refresh battery (silent) */
            int fd = open(g_hidraw_path, O_RDWR);
            if (fd >= 0) {
                unsigned char buf[65] = {0};
                buf[0] = 0x00;
                buf[1] = 0xF7;
                ioctl(fd, HIDIOCSFEATURE(65), buf);
                close(fd);
            }
            seconds_since_f7 = 0;
        }
    }

    fprintf(stderr, "\nUnloading BPF program...\n");
    akko_bidirectional_bpf__destroy(skel);
    fprintf(stderr, "Done\n");

    return 0;
}
