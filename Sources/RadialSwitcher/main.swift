import AppKit

setvbuf(stdout, nil, _IOLBF, 0)

let arguments = Array(CommandLine.arguments.dropFirst())

if arguments.isEmpty {
    let app = NSApplication.shared
    let delegate = AppDelegate()
    app.delegate = delegate
    app.setActivationPolicy(.accessory)
    app.run()
} else {
    Task {
        exit(await DebugCLI.run(arguments))
    }
    RunLoop.main.run()
}
