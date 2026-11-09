// Adds Workshop's launcher entry on Linux. The file name is Tauri's
// identifier, which GNOME on Wayland matches a window's application id
// against.

function Component() {}

// A quoted desktop-entry Exec argument, escaped as the Gateway script's
// `desktopExec` explains: Exec quoting, then the string escape, and `%`
// doubled.
function desktopExec(path) {
    var quoted = "\"" + path.replace(/["`$\\]/g, "\\$&") + "\"";
    return quoted.replace(/\\/g, "\\\\").replace(/%/g, "%%");
}

Component.prototype.createOperations = function () {
    component.createOperations();

    if (systemInfo.kernelType === "linux") {
        var target = installer.value("TargetDir");
        component.addOperation("CreateDesktopEntry", "com.runvil.workshop.desktop", [
            "Type=Application",
            "Name=Runvil",
            "Comment=Write and run prompts",
            "Exec=" + desktopExec(target + "/{{WORKSHOP_EXECUTABLE}}"),
            "Icon=" + target + "/runvil-workshop.png",
            "Terminal=false",
            "Categories=Development;"
        ].join("\n"));
    }
};
