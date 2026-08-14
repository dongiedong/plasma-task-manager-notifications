function reportWindow(window) {
    if (!window)
        return;

    var desktopFile = window.desktopFileName;
    if (!desktopFile)
        return;

    callDBus(
        "org.kde.PlasmaTaskManagerNotifications",
        "/org/kde/PlasmaTaskManagerNotifications",
        "org.kde.PlasmaTaskManagerNotifications",
        "FocusApp",
        desktopFile
    );
}

workspace.windowActivated.connect(reportWindow);
