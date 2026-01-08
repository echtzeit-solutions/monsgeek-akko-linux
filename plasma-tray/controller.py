#!/usr/bin/env python3
"""Controller for MonsGeek/Akko keyboard - wraps iot_driver CLI"""

import subprocess
import re
import shutil

# Try PySide6 first, fall back to PyQt6
try:
    from PySide6.QtCore import QObject, Signal, Slot, Property
except ImportError:
    from PyQt6.QtCore import QObject, pyqtSignal as Signal, pyqtSlot as Slot, pyqtProperty as Property

# LED mode definitions
LED_MODES = [
    (0, "Off"),
    (1, "Constant"),
    (2, "Breathing"),
    (3, "Neon"),
    (4, "Wave"),
    (5, "Ripple"),
    (6, "Raindrop"),
    (7, "Snake"),
    (8, "Reactive"),
    (9, "Converge"),
    (10, "Sine Wave"),
    (11, "Kaleidoscope"),
    (12, "Line Wave"),
    (13, "User Picture"),
    (14, "Laser"),
    (15, "Circle Wave"),
    (16, "Rainbow"),
    (17, "Rain Down"),
    (18, "Meteor"),
    (19, "Reactive Off"),
    (20, "Music Patterns"),
    (21, "Screen Sync"),
    (22, "Music Bars"),
    (23, "Train"),
    (24, "Fireworks"),
    (25, "Per-Key Color"),
]


def find_iot_driver():
    """Find iot_driver binary in PATH or common locations"""
    # Check PATH first
    path = shutil.which("iot_driver")
    if path:
        return path

    # Check common locations
    for loc in [
        "/usr/local/bin/iot_driver",
        "/usr/bin/iot_driver",
        "~/.local/bin/iot_driver",
        "./iot_driver_linux/target/release/iot_driver",
    ]:
        import os
        expanded = os.path.expanduser(loc)
        if os.path.isfile(expanded) and os.access(expanded, os.X_OK):
            return expanded

    return "iot_driver"  # Fall back, will fail if not found


class KeyboardController(QObject):
    """Controller that interfaces with iot_driver CLI"""

    # Signals for state changes
    modeChanged = Signal(int)
    brightnessChanged = Signal(int)
    profileChanged = Signal(int)
    connectedChanged = Signal(bool)
    statusChanged = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self._mode = 0
        self._brightness = 4
        self._speed = 2
        self._profile = 0
        self._connected = False
        self._status = "Disconnected"
        self._binary = find_iot_driver()
        self._r = 255
        self._g = 255
        self._b = 255

    def _run(self, *args):
        """Run iot_driver command and return output"""
        try:
            result = subprocess.run(
                [self._binary] + list(args),
                capture_output=True,
                text=True,
                timeout=5
            )
            return result.stdout, result.returncode == 0
        except subprocess.TimeoutExpired:
            return "", False
        except FileNotFoundError:
            self._status = "iot_driver not found"
            self.statusChanged.emit(self._status)
            return "", False

    @Slot()
    def refresh(self):
        """Refresh current state from device"""
        # Get LED state
        output, ok = self._run("led")
        if ok and output:
            self._connected = True
            self._parse_led_output(output)
        else:
            self._connected = False
            self._status = "Disconnected"

        # Get profile
        output, ok = self._run("profile")
        if ok and output:
            match = re.search(r'Profile:\s*(\d+)', output)
            if match:
                self._profile = int(match.group(1))
                self.profileChanged.emit(self._profile)

        self.connectedChanged.emit(self._connected)
        self.statusChanged.emit(self._status)

    def _parse_led_output(self, output):
        """Parse LED command output"""
        # Mode: 2 (Breathing)
        match = re.search(r'Mode:\s*(\d+)', output)
        if match:
            self._mode = int(match.group(1))
            self.modeChanged.emit(self._mode)

        # Brightness: 4
        match = re.search(r'Brightness:\s*(\d+)', output)
        if match:
            self._brightness = int(match.group(1))
            self.brightnessChanged.emit(self._brightness)

        # Speed: 2
        match = re.search(r'Speed:\s*(\d+)', output)
        if match:
            self._speed = int(match.group(1))

        # Color: RGB(255, 255, 255)
        match = re.search(r'RGB\((\d+),\s*(\d+),\s*(\d+)\)', output)
        if match:
            self._r = int(match.group(1))
            self._g = int(match.group(2))
            self._b = int(match.group(3))

        mode_name = LED_MODES[self._mode][1] if self._mode < len(LED_MODES) else "Unknown"
        self._status = f"{mode_name} | Brightness {self._brightness}"

    @Slot(int)
    def setMode(self, mode: int):
        """Set LED mode"""
        output, ok = self._run("mode", str(mode))
        if ok:
            self._mode = mode
            self.modeChanged.emit(mode)
            mode_name = LED_MODES[mode][1] if mode < len(LED_MODES) else "Unknown"
            self._status = f"{mode_name} | Brightness {self._brightness}"
            self.statusChanged.emit(self._status)

    @Slot(int)
    def setBrightness(self, brightness: int):
        """Set LED brightness (0-4)"""
        # Use set-led command: mode brightness speed r g b
        output, ok = self._run(
            "set-led",
            str(self._mode),
            str(brightness),
            str(self._speed),
            str(self._r),
            str(self._g),
            str(self._b)
        )
        if ok:
            self._brightness = brightness
            self.brightnessChanged.emit(brightness)
            mode_name = LED_MODES[self._mode][1] if self._mode < len(LED_MODES) else "Unknown"
            self._status = f"{mode_name} | Brightness {brightness}"
            self.statusChanged.emit(self._status)

    @Slot(int)
    def setProfile(self, profile: int):
        """Set active profile (0-3)"""
        output, ok = self._run("set-profile", str(profile))
        if ok:
            self._profile = profile
            self.profileChanged.emit(profile)

    @Slot()
    def openTui(self):
        """Open TUI in terminal"""
        import os
        # Try common terminal emulators
        terminals = ["konsole", "gnome-terminal", "xterm", "alacritty", "kitty"]
        for term in terminals:
            if shutil.which(term):
                if term == "konsole":
                    subprocess.Popen([term, "-e", self._binary, "tui"])
                elif term == "gnome-terminal":
                    subprocess.Popen([term, "--", self._binary, "tui"])
                else:
                    subprocess.Popen([term, "-e", f"{self._binary} tui"])
                return

    # Properties for QML
    @Property(int, notify=modeChanged)
    def mode(self):
        return self._mode

    @Property(int, notify=brightnessChanged)
    def brightness(self):
        return self._brightness

    @Property(int, notify=profileChanged)
    def profile(self):
        return self._profile

    @Property(bool, notify=connectedChanged)
    def connected(self):
        return self._connected

    @Property(str, notify=statusChanged)
    def status(self):
        return self._status
