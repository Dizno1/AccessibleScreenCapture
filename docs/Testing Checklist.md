# Testing Checklist - 1.0.1 (Configurable Shortcuts + Capture Context Descriptor)

1.0.0 built successfully via GitHub Actions and was installed and verified on Windows. None of what follows - the three-shortcut rework or the new Capture Context Descriptor - has been through that same verified build yet. This is the checklist for once a real 1.0.1 build exists.

## Global shortcuts

- [ ] Alt+Ctrl+Space takes a screenshot while Word has focus.
- [ ] Alt+Ctrl+Space takes a screenshot while Chrome has focus.
- [ ] Alt+Ctrl+R starts a recording while another application has focus, and stops it the same way.
- [ ] Alt+Ctrl+D turns the Capture Context Descriptor on while another application has focus, without switching focus to AccessibleScreenCapture.
- [ ] All three shortcuts work with the AccessibleScreenCapture window minimized to tray.

## Shortcut registration and conflicts

- [ ] Changing any one of the three shortcuts to a free combination registers it immediately and announces "<Name> shortcut <combo> registered."
- [ ] Escape while waiting for a new combination cancels without changing anything.
- [ ] Assigning one shortcut's current combination to either of the other two is rejected before anything is unregistered, names which shortcut and that it's already assigned to another shortcut, and the previous combination still works immediately afterward.
- [ ] Assigning a combination already claimed by another application is rejected, the previous shortcut is restored and still works, and the message names which shortcut failed and that another application is using it.
- [ ] Restore Defaults resets all three shortcuts to Alt+Ctrl+Space / Alt+Ctrl+R / Alt+Ctrl+D in one step and announces the result.
- [ ] A shortcut that fails to register at startup (e.g. a previously-saved combination now claimed by another application) is announced by name, not with a generic message.
- [ ] Every step above is reachable and completable with the keyboard only.

## Persistence

- [ ] A changed shortcut survives a full app restart (not just minimizing to tray).
- [ ] A `shortcuts.json` saved by 1.0.0 (no descriptor entry) loads correctly on 1.0.1, gets the Alt+Ctrl+D default, and does not reset the screenshot/recording shortcuts if they were previously customized.
- [ ] The Capture Context Descriptor's on/off state does NOT persist across a restart - it should always start off, even if it was left on at exit.

## Capture Context Descriptor - independence from capture

- [ ] Taking a screenshot or starting a recording does not turn the descriptor on by itself.
- [ ] Turning the descriptor on, then taking a screenshot, does not produce a duplicate or capture-specific description - only the descriptor's own change-based announcements occur.
- [ ] The descriptor keeps running (or stays off) regardless of Review/Save/Discard state.

## Capture Context Descriptor - window states and changes

- [ ] Turning the descriptor on immediately describes the current active window once.
- [ ] Alt-Tabbing to a different application announces the new application and window.
- [ ] Maximized window: announces app, title, "Maximized," and that it fills the available screen.
- [ ] Restored window snapped to half the screen (Win+Left/Right): announces "Restored," then the correct half and monitor.
- [ ] Restored window snapped to a quarter (Windows Snap corner): announces the correct quadrant.
- [ ] Full-screen application (game or video player): announces "Full screen" and the monitor, without a redundant separate fill/no-fill sentence.
- [ ] Minimized window as the active window (edge case): announces app name and "Minimized," nothing else.
- [ ] A window extending off its monitor: announces that it extends beyond the visible desktop.
- [ ] Switching back and forth between two windows repeatedly does not repeat identical descriptions - only real changes are announced.
- [ ] Rapid Alt+Tab through several windows quickly settles into at most one announcement, not one per keystroke.
- [ ] Turning the descriptor off stops announcements within about half a second, with no further speech.

## Multiple monitors

- [ ] Active window on the second monitor reports the correct monitor number.
- [ ] Monitor numbering is at least internally consistent across repeated checks in one session - note whether it matches Windows Display Settings' own numbering (known unverified assumption).

## Descriptor and screen reader interaction

- [ ] The descriptor never announces document text, webpage text, control contents, or focus-change chatter - only application/window/monitor state.
- [ ] With JAWS/NVDA/Narrator running and actively reading content, the descriptor's announcements are distinguishable from and don't talk over normal screen reader navigation.

## Regression (should be unaffected by this pass)

- [ ] Screenshot and recording Review/Save/Discard workflow is unchanged.
- [ ] Recent Captures is unchanged.
- [ ] Native notifications for capture/save/discard events still fire correctly when the window is hidden.
- [ ] System tray, minimize-to-tray, and Quit are unchanged.
