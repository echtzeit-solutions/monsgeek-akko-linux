# Option C: Pure Feature Report on Vendor Interface

**Status: DOES NOT WORK**

This approach attempted to use only a Feature report (0xB1) on the vendor interface without requiring an input device.

## Concept

The idea was that a Feature report for Battery Strength would be polled by the kernel via `hid_hw_raw_request()`, and the power_supply would be created without needing an input device.

## Why It Doesn't Work

The kernel's `hidinput_connect()` function is responsible for creating both input devices and power_supply entries. It only runs when:

1. The HID descriptor contains mappable input usages (keys, buttons, axes, etc.)
2. An input device is created

Without an input device, the battery detection code path is never reached.

## Descriptor Used

```c
static const __u8 battery_rdesc[] = {
    0x05, 0x01,        /* Usage Page (Generic Desktop) */
    0x09, 0x06,        /* Usage (Keyboard) */
    0xA1, 0x01,        /* Collection (Application) */
    0x85, 0x05,        /*   Report ID (5) */
    0x05, 0x06,        /*   Usage Page (Generic Device Controls) */
    0x09, 0x20,        /*   Usage (Battery Strength) */
    0x15, 0x00,        /*   Logical Minimum (0) */
    0x26, 0x64, 0x00,  /*   Logical Maximum (100) */
    0x75, 0x08,        /*   Report Size (8) */
    0x95, 0x01,        /*   Report Count (1) */
    0xB1, 0x02,        /*   Feature (Data,Var,Abs) */
    0xC0               /* End Collection */
};
```

## Test Result

After loading this BPF and rebinding the device:
- Descriptor was correctly modified to 24 bytes
- Feature report was present in descriptor
- **No power_supply created** - `ls /sys/class/power_supply/ | grep hid` shows only other devices

## Conclusion

The kernel requires an input device to exist before it will create power_supply entries for HID batteries. Pure Feature reports are not sufficient.

## Alternative

Use the "working" solution which adds a dummy input control to force input device creation.
