// SPDX-License-Identifier: GPL-2.0
/*
 * HID-BPF loader for Akko keyboard battery integration
 * Option B WQ: Experimental bpf_wq version
 *
 * This loader is for the experimental bpf_wq implementation that
 * attempts to send F7 commands from within BPF automatically.
 *
 * Usage: sudo ./loader_wq [hid_id]
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

#include "akko_wq.skel.h"

#define VID 0x3151
#define PID 0x5038

static volatile int running = 1;

static void sig_handler(int sig)
{
    (void)sig;
    running = 0;
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

/* Send F7 command to prime battery cache */
static int send_f7_command(const char *hidraw_path)
{
    int fd;
    unsigned char buf[65] = {0};
    int ret;

    fprintf(stderr, "Sending initial F7 command...\n");

    fd = open(hidraw_path, O_RDWR);
    if (fd < 0) {
        fprintf(stderr, "  Failed to open hidraw: %s\n", strerror(errno));
        return -1;
    }

    buf[0] = 0x00;
    buf[1] = 0xF7;

    ret = ioctl(fd, HIDIOCSFEATURE(65), buf);
    if (ret < 0) {
        fprintf(stderr, "  SET_FEATURE failed: %s\n", strerror(errno));
        close(fd);
        return -1;
    }

    usleep(100000);

    buf[0] = 0x00;
    ret = ioctl(fd, HIDIOCGFEATURE(65), buf);
    if (ret >= 0 && buf[1] > 0 && buf[1] <= 100) {
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
    fprintf(stderr, "Device rebound\n");
}

static char g_device_name[64];

/* Find vendor HID interface */
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

        if (sscanf(ent->d_name, "%x:%x:%x.%x", &bus, &vid, &pid, &id) != 4)
            continue;

        if (vid != VID || pid != PID)
            continue;

        fprintf(stderr, "  Checking %s...\n", ent->d_name);

        snprintf(path, sizeof(path), "/sys/bus/hid/devices/%s/report_descriptor",
                 ent->d_name);
        f = fopen(path, "rb");
        if (!f)
            continue;

        size_t len = fread(rdesc, 1, sizeof(rdesc), f);
        fclose(f);

        fprintf(stderr, "    Descriptor size=%zu, first bytes: %02x %02x %02x\n",
                len, rdesc[0], rdesc[1], rdesc[2]);

        if (len >= 3 && rdesc[0] == 0x06 && rdesc[1] == 0xFF && rdesc[2] == 0xFF) {
            fprintf(stderr, "Found vendor interface: %s (hid_id=%u)\n", ent->d_name, id);
            strncpy(g_device_name, ent->d_name, sizeof(g_device_name) - 1);
            hid_id = id;

            char *hidraw = find_hidraw_for_hid(ent->d_name);
            if (hidraw) {
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
    struct akko_wq_bpf *skel;
    int hid_id;
    int err;

    fprintf(stderr, "Akko Keyboard Battery BPF loader (Option B WQ - EXPERIMENTAL)\n");
    fprintf(stderr, "Using bpf_wq for automatic F7 refresh\n\n");

    if (geteuid() != 0) {
        fprintf(stderr, "Error: Must run as root\n");
        return 1;
    }

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
            return 1;
        }
    }

    fprintf(stderr, "Opening BPF skeleton...\n");
    skel = akko_wq_bpf__open();
    if (!skel) {
        fprintf(stderr, "Failed to open BPF skeleton: %s\n", strerror(errno));
        return 1;
    }

    fprintf(stderr, "Setting hid_id=%d in struct_ops...\n", hid_id);
    skel->struct_ops.akko_wq->hid_id = hid_id;

    fprintf(stderr, "Loading BPF programs...\n");
    err = akko_wq_bpf__load(skel);
    if (err) {
        fprintf(stderr, "Failed to load BPF: %s\n", strerror(-err));
        akko_wq_bpf__destroy(skel);
        return 1;
    }
    fprintf(stderr, "BPF loaded successfully\n");

    fprintf(stderr, "Attaching struct_ops...\n");
    err = akko_wq_bpf__attach(skel);
    if (err) {
        fprintf(stderr, "Failed to attach BPF: %s\n", strerror(-err));
        akko_wq_bpf__destroy(skel);
        return 1;
    }

    fprintf(stderr, "BPF program loaded and attached!\n");

    if (g_device_name[0]) {
        rebind_hid_device(g_device_name);

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
    fprintf(stderr, "bpf_wq should auto-refresh F7 every 30s (check trace_pipe)\n");

    signal(SIGINT, sig_handler);
    signal(SIGTERM, sig_handler);

    while (running) {
        sleep(1);
    }

    fprintf(stderr, "\nUnloading BPF program...\n");
    akko_wq_bpf__destroy(skel);
    fprintf(stderr, "Done\n");

    return 0;
}
