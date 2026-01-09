// SPDX-License-Identifier: GPL-2.0
/*
 * HID-BPF loader for Akko keyboard battery integration (Option A - RECOMMENDED)
 *
 * Loads BPF that attaches to keyboard interface (00D8) to inject
 * battery Feature report. The dongle firmware handles battery queries
 * directly - no userspace polling or BPF maps needed!
 *
 * Usage: sudo ./loader_kb [hid_id]
 *   If hid_id not specified, auto-detects keyboard interface
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <dirent.h>
#include <signal.h>
#include <sys/types.h>

#include "akko_keyboard_battery.skel.h"

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

/* Find keyboard HID interface (starts with 05 01 09 06) */
static int find_keyboard_interface(void)
{
    DIR *dir;
    struct dirent *ent;
    char path[512];
    unsigned char rdesc[64];
    FILE *f;
    int hid_id = -1;

    fprintf(stderr, "Searching for keyboard interface VID=%04x PID=%04x...\n", VID, PID);

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

        /* Check descriptor - keyboard interface starts with 05 01 09 06 */
        snprintf(path, sizeof(path), "/sys/bus/hid/devices/%s/report_descriptor",
                 ent->d_name);
        f = fopen(path, "rb");
        if (!f)
            continue;

        size_t len = fread(rdesc, 1, sizeof(rdesc), f);
        fclose(f);

        fprintf(stderr, "    Descriptor size=%zu, first bytes: %02x %02x %02x %02x\n",
                len, rdesc[0], rdesc[1], rdesc[2], rdesc[3]);

        /* Check for keyboard interface: 05 01 09 06 */
        if (len >= 4 &&
            rdesc[0] == 0x05 && rdesc[1] == 0x01 &&
            rdesc[2] == 0x09 && rdesc[3] == 0x06) {
            fprintf(stderr, "Found keyboard interface: %s (hid_id=%u)\n", ent->d_name, id);
            strncpy(g_device_name, ent->d_name, sizeof(g_device_name) - 1);
            hid_id = id;
            break;
        }
    }

    closedir(dir);
    return hid_id;
}

int main(int argc, char **argv)
{
    struct akko_keyboard_battery_bpf *skel;
    int hid_id;
    int err;

    fprintf(stderr, "Akko Keyboard Battery BPF loader (Option A - Recommended)\n");
    fprintf(stderr, "Firmware responds to Feature Report 5 on any interface!\n\n");

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
        hid_id = find_keyboard_interface();
        if (hid_id < 0) {
            fprintf(stderr, "Could not find keyboard interface\n");
            fprintf(stderr, "Make sure the dongle is connected\n");
            return 1;
        }
    }

    /* Open BPF skeleton */
    fprintf(stderr, "Opening BPF skeleton...\n");
    skel = akko_keyboard_battery_bpf__open();
    if (!skel) {
        fprintf(stderr, "Failed to open BPF skeleton: %s\n", strerror(errno));
        return 1;
    }

    /* Set hid_id BEFORE loading */
    fprintf(stderr, "Setting hid_id=%d in struct_ops...\n", hid_id);
    skel->struct_ops.akko_keyboard_battery->hid_id = hid_id;

    /* Load BPF programs */
    fprintf(stderr, "Loading BPF programs...\n");
    err = akko_keyboard_battery_bpf__load(skel);
    if (err) {
        fprintf(stderr, "Failed to load BPF: %s\n", strerror(-err));
        akko_keyboard_battery_bpf__destroy(skel);
        return 1;
    }
    fprintf(stderr, "BPF loaded successfully\n");

    /* Attach struct_ops */
    fprintf(stderr, "Attaching struct_ops...\n");
    err = akko_keyboard_battery_bpf__attach(skel);
    if (err) {
        fprintf(stderr, "Failed to attach BPF: %s\n", strerror(-err));
        akko_keyboard_battery_bpf__destroy(skel);
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

    /* Handle signals for clean shutdown */
    signal(SIGINT, sig_handler);
    signal(SIGTERM, sig_handler);

    /* Keep running to maintain the BPF link */
    while (running) {
        sleep(1);
    }

    fprintf(stderr, "\nUnloading BPF program...\n");
    akko_keyboard_battery_bpf__destroy(skel);
    fprintf(stderr, "Done\n");

    return 0;
}
