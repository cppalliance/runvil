// Adds the Linux desktop entry that opens the Gateway's Settings page. The
// Gateway writes its default configuration, speech included, the first
// time it starts.

function Component() {}

// A quoted desktop-entry Exec argument, in the spec's two layers: the
// reserved characters backslash-escaped inside the quotes, then every
// backslash doubled for the string escape, which readers undo first and
// which rejects `\$` and `\"`. IFW writes the line verbatim. `%` is doubled,
// since it starts a field code anywhere on the line.
function desktopExec(path) {
    var quoted = "\"" + path.replace(/["`$\\]/g, "\\$&") + "\"";
    return quoted.replace(/\\/g, "\\\\").replace(/%/g, "%%");
}

Component.prototype.createOperations = function () {
    component.createOperations();

    var target = installer.value("TargetDir");
    var gateway = target + "/{{GATEWAY_EXECUTABLE}}";
    if (systemInfo.kernelType === "linux") {
        component.addOperation("CreateDesktopEntry", "com.runvil.gateway.desktop", [
            "Type=Application",
            "Name=Runvil Gateway Settings",
            "Comment=Open the Runvil Gateway Settings page",
            "Exec=" + desktopExec(gateway) + " --browser",
            "Icon=" + target + "/runvil-gateway.png",
            "Terminal=false",
            "Categories=Development;"
        ].join("\n"));
    }
};
