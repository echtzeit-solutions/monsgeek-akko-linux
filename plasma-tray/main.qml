import QtQuick
import QtQuick.Controls
import Qt.labs.platform as Platform

Item {
    id: root

    Platform.SystemTrayIcon {
        id: trayIcon
        visible: true
        icon.name: "input-keyboard"
        tooltip: controller.connected ? controller.status : "Akko Keyboard - Disconnected"

        onActivated: function(reason) {
            // Refresh state and open menu on any click
            controller.refresh()
            menu.open()
        }

        menu: Platform.Menu {
            id: menu

            Platform.MenuItem {
                text: controller.connected ? ("● " + controller.status) : "○ Disconnected"
                enabled: false
            }

            Platform.MenuSeparator {}

            // === LED Modes (common ones at top level) ===
            Platform.MenuItem {
                text: "LED: Off"
                checkable: true
                checked: controller.mode === 0
                onTriggered: controller.setMode(0)
            }
            Platform.MenuItem {
                text: "LED: Breathing"
                checkable: true
                checked: controller.mode === 2
                onTriggered: controller.setMode(2)
            }
            Platform.MenuItem {
                text: "LED: Wave"
                checkable: true
                checked: controller.mode === 4
                onTriggered: controller.setMode(4)
            }
            Platform.MenuItem {
                text: "LED: Rainbow"
                checkable: true
                checked: controller.mode === 16
                onTriggered: controller.setMode(16)
            }
            Platform.MenuItem {
                text: "LED: Reactive"
                checkable: true
                checked: controller.mode === 8
                onTriggered: controller.setMode(8)
            }

            Platform.Menu {
                title: "More LED Modes..."

                Platform.MenuItem { text: "Constant"; checkable: true; checked: controller.mode === 1; onTriggered: controller.setMode(1) }
                Platform.MenuItem { text: "Neon"; checkable: true; checked: controller.mode === 3; onTriggered: controller.setMode(3) }
                Platform.MenuItem { text: "Ripple"; checkable: true; checked: controller.mode === 5; onTriggered: controller.setMode(5) }
                Platform.MenuItem { text: "Raindrop"; checkable: true; checked: controller.mode === 6; onTriggered: controller.setMode(6) }
                Platform.MenuItem { text: "Snake"; checkable: true; checked: controller.mode === 7; onTriggered: controller.setMode(7) }
                Platform.MenuItem { text: "Converge"; checkable: true; checked: controller.mode === 9; onTriggered: controller.setMode(9) }
                Platform.MenuItem { text: "Sine Wave"; checkable: true; checked: controller.mode === 10; onTriggered: controller.setMode(10) }
                Platform.MenuItem { text: "Kaleidoscope"; checkable: true; checked: controller.mode === 11; onTriggered: controller.setMode(11) }
                Platform.MenuItem { text: "Line Wave"; checkable: true; checked: controller.mode === 12; onTriggered: controller.setMode(12) }
                Platform.MenuItem { text: "User Picture"; checkable: true; checked: controller.mode === 13; onTriggered: controller.setMode(13) }
                Platform.MenuItem { text: "Laser"; checkable: true; checked: controller.mode === 14; onTriggered: controller.setMode(14) }
                Platform.MenuItem { text: "Circle Wave"; checkable: true; checked: controller.mode === 15; onTriggered: controller.setMode(15) }
                Platform.MenuItem { text: "Rain Down"; checkable: true; checked: controller.mode === 17; onTriggered: controller.setMode(17) }
                Platform.MenuItem { text: "Meteor"; checkable: true; checked: controller.mode === 18; onTriggered: controller.setMode(18) }
                Platform.MenuItem { text: "Reactive Off"; checkable: true; checked: controller.mode === 19; onTriggered: controller.setMode(19) }
                Platform.MenuItem { text: "Music Patterns"; checkable: true; checked: controller.mode === 20; onTriggered: controller.setMode(20) }
                Platform.MenuItem { text: "Screen Sync"; checkable: true; checked: controller.mode === 21; onTriggered: controller.setMode(21) }
                Platform.MenuItem { text: "Music Bars"; checkable: true; checked: controller.mode === 22; onTriggered: controller.setMode(22) }
                Platform.MenuItem { text: "Train"; checkable: true; checked: controller.mode === 23; onTriggered: controller.setMode(23) }
                Platform.MenuItem { text: "Fireworks"; checkable: true; checked: controller.mode === 24; onTriggered: controller.setMode(24) }
                Platform.MenuItem { text: "Per-Key Color"; checkable: true; checked: controller.mode === 25; onTriggered: controller.setMode(25) }
            }

            Platform.MenuSeparator {}

            // === Brightness ===
            Platform.Menu {
                title: "Brightness: " + controller.brightness

                Platform.MenuItem { text: "0 (Off)"; checkable: true; checked: controller.brightness === 0; onTriggered: controller.setBrightness(0) }
                Platform.MenuItem { text: "1"; checkable: true; checked: controller.brightness === 1; onTriggered: controller.setBrightness(1) }
                Platform.MenuItem { text: "2"; checkable: true; checked: controller.brightness === 2; onTriggered: controller.setBrightness(2) }
                Platform.MenuItem { text: "3"; checkable: true; checked: controller.brightness === 3; onTriggered: controller.setBrightness(3) }
                Platform.MenuItem { text: "4 (Max)"; checkable: true; checked: controller.brightness === 4; onTriggered: controller.setBrightness(4) }
            }

            // === Profile ===
            Platform.Menu {
                title: "Profile: " + (controller.profile + 1)

                Platform.MenuItem { text: "Profile 1"; checkable: true; checked: controller.profile === 0; onTriggered: controller.setProfile(0) }
                Platform.MenuItem { text: "Profile 2"; checkable: true; checked: controller.profile === 1; onTriggered: controller.setProfile(1) }
                Platform.MenuItem { text: "Profile 3"; checkable: true; checked: controller.profile === 2; onTriggered: controller.setProfile(2) }
                Platform.MenuItem { text: "Profile 4"; checkable: true; checked: controller.profile === 3; onTriggered: controller.setProfile(3) }
            }

            Platform.MenuSeparator {}

            Platform.MenuItem {
                text: "Refresh"
                shortcut: "R"
                onTriggered: controller.refresh()
            }

            Platform.MenuItem {
                text: "Open TUI..."
                onTriggered: controller.openTui()
            }

            Platform.MenuSeparator {}

            Platform.MenuItem {
                text: "Quit"
                shortcut: "Q"
                onTriggered: Qt.quit()
            }
        }
    }
}
