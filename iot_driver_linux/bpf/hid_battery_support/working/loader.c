// SPDX-License-Identifier: GPL-2.0
/*
 * HID-BPF loader for Akko dongle battery integration
 *
 * Uses libbpf skeleton to properly set hid_id before loading.
 *
 * Usage: sudo ./loader [hid_id]
 *   If hid_id not specified, auto-detects from VID:PID
 *
 * Build: cc -o loader loader.c -lbpf
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <unistd.h>
#include <dirent.h>
#include <signal.h>
#include <sys/types.h>

#include "akko_dongle.skel.h"

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

        /* Check if this is another loader instance (just check for "loader") */
        if (strstr(cmdline, "loader")) {
            fprintf(stderr, "Killing previous loader (PID %d)...\n", pid);
            kill(pid, SIGTERM);
            killed++;
        }
    }

    closedir(dir);

    if (killed > 0) {
        /* Give it time to clean up */
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

    usleep(100000); /* 100ms */

    /* Rebind to hid-generic */
    snprintf(path, sizeof(path), "/sys/bus/hid/drivers/hid-generic/bind");
    f = fopen(path, "w");
    if (f) {
        fprintf(f, "%s", device_name);
        fclose(f);
    }

    usleep(100000); /* 100ms */
    fprintf(stderr, "Device rebound\n");
}

/* Global to store device name for rebind */
static char g_device_name[64];

/* Find HID device matching VID:PID with small descriptor (vendor interface) */
static int find_hid_device(void)
{
    DIR *dir;
    struct dirent *ent;
    char path[512];
    unsigned char rdesc[32];
    FILE *f;
    int hid_id = -1;

    fprintf(stderr, "Searching for HID device VID=%04x PID=%04x...\n", VID, PID);

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

        /*
         * Check for:
         * - Original vendor page (06 FF FF) with small size, OR
         * - Already-modified battery descriptor (05 01 = Generic Desktop)
         */
        int is_original = (len >= 3 && len <= 24 &&
                          rdesc[0] == 0x06 && rdesc[1] == 0xFF && rdesc[2] == 0xFF);
        int is_modified = (len >= 3 && len <= 48 &&
                          rdesc[0] == 0x05 && rdesc[1] == 0x01);

        if (is_original || is_modified) {
            fprintf(stderr, "Found target device: %s (hid_id=%u)%s\n",
                    ent->d_name, id, is_modified ? " [already modified]" : "");
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
    struct akko_dongle_bpf *skel;
    int hid_id;
    int err;

    fprintf(stderr, "Akko HID-BPF loader starting...\n");

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
        hid_id = find_hid_device();
        if (hid_id < 0) {
            fprintf(stderr, "Could not find target HID device\n");
            fprintf(stderr, "Make sure the dongle is connected\n");
            return 1;
        }
    }

    /* Open BPF skeleton */
    fprintf(stderr, "Opening BPF skeleton...\n");
    skel = akko_dongle_bpf__open();
    if (!skel) {
        fprintf(stderr, "Failed to open BPF skeleton: %s\n", strerror(errno));
        return 1;
    }

    /* Set hid_id BEFORE loading - this is the key! */
    fprintf(stderr, "Setting hid_id=%d in struct_ops...\n", hid_id);
    skel->struct_ops.akko_dongle->hid_id = hid_id;

    /* Load BPF programs and maps */
    fprintf(stderr, "Loading BPF programs...\n");
    err = akko_dongle_bpf__load(skel);
    if (err) {
        fprintf(stderr, "Failed to load BPF: %s\n", strerror(-err));
        akko_dongle_bpf__destroy(skel);
        return 1;
    }
    fprintf(stderr, "BPF loaded successfully\n");

    /* Attach struct_ops */
    fprintf(stderr, "Attaching struct_ops...\n");
    err = akko_dongle_bpf__attach(skel);
    if (err) {
        fprintf(stderr, "Failed to attach BPF: %s\n", strerror(-err));
        akko_dongle_bpf__destroy(skel);
        return 1;
    }

    fprintf(stderr, "BPF program loaded and attached successfully!\n");

    /* Rebind the device to trigger descriptor re-parse */
    if (g_device_name[0]) {
        rebind_hid_device(g_device_name);

        /* Check if power_supply was created */
        usleep(500000); /* Give kernel time to set up */
        char ps_path[256];
        snprintf(ps_path, sizeof(ps_path), "/sys/bus/hid/devices/%s/power_supply", g_device_name);
        if (access(ps_path, F_OK) == 0) {
            fprintf(stderr, "Power supply created successfully!\n");
        } else {
            /* Also check /sys/class/power_supply for hid-* entries */
            DIR *dir = opendir("/sys/class/power_supply");
            if (dir) {
                struct dirent *ent;
                while ((ent = readdir(dir)) != NULL) {
                    if (strstr(ent->d_name, "hid-") && strstr(ent->d_name, "3151")) {
                        fprintf(stderr, "Power supply found: %s\n", ent->d_name);
                        break;
                    }
                }
                closedir(dir);
            }
        }

        /* Check if input device was created */
        snprintf(ps_path, sizeof(ps_path), "/sys/bus/hid/devices/%s/input", g_device_name);
        if (access(ps_path, F_OK) == 0) {
            fprintf(stderr, "Input device created!\n");
        } else {
            fprintf(stderr, "Warning: No input device created\n");
        }
    }

    fprintf(stderr, "Press Ctrl+C to unload...\n");

    /* Handle signals for clean shutdown */
    signal(SIGINT, sig_handler);
    signal(SIGTERM, sig_handler);

    /* Keep running to maintain the BPF link */
    while (running) {
        sleep(1);
    }

    fprintf(stderr, "\nUnloading BPF program...\n");
    akko_dongle_bpf__destroy(skel);
    fprintf(stderr, "Done\n");

    return 0;
}
