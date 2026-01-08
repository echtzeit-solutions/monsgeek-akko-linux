#!/usr/bin/env python3
"""MonsGeek/Akko Keyboard Tray Application for KDE Plasma"""

import sys
import os
from pathlib import Path

# Try PySide6 first, fall back to PyQt6
try:
    from PySide6.QtCore import QUrl, QTimer
    from PySide6.QtWidgets import QApplication
    from PySide6.QtQml import QQmlApplicationEngine
except ImportError:
    from PyQt6.QtCore import QUrl, QTimer
    from PyQt6.QtWidgets import QApplication
    from PyQt6.QtQml import QQmlApplicationEngine

from controller import KeyboardController, LED_MODES


def main():
    # Set app metadata
    QApplication.setApplicationName("Akko Keyboard")
    QApplication.setOrganizationName("akko-keyboard")
    QApplication.setDesktopFileName("akko-tray")

    app = QApplication(sys.argv)

    # Don't quit when last window closes (we're a tray app)
    app.setQuitOnLastWindowClosed(False)

    # Create controller
    controller = KeyboardController()

    # Create QML engine
    engine = QQmlApplicationEngine()

    # Expose controller and LED modes to QML
    engine.rootContext().setContextProperty("controller", controller)
    engine.rootContext().setContextProperty("ledModes", [
        {"id": m[0], "name": m[1]} for m in LED_MODES
    ])

    # Find QML file
    qml_file = Path(__file__).parent / "main.qml"
    if not qml_file.exists():
        # Try installed location
        qml_file = Path("/usr/share/akko-keyboard/tray/main.qml")

    engine.load(QUrl.fromLocalFile(str(qml_file)))

    if not engine.rootObjects():
        print("Failed to load QML", file=sys.stderr)
        return 1

    # Initial refresh
    controller.refresh()

    # Periodic refresh every 30 seconds
    timer = QTimer()
    timer.timeout.connect(controller.refresh)
    timer.start(30000)

    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
