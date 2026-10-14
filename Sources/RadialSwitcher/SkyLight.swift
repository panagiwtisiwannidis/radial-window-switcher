import ApplicationServices
import Foundation

enum SkyLight {
    private typealias SetFrontProcessFn = @convention(c) (UnsafeMutablePointer<ProcessSerialNumber>, CGWindowID, UInt32) -> CGError
    private typealias PostEventRecordFn = @convention(c) (UnsafeMutablePointer<ProcessSerialNumber>, UnsafeMutablePointer<UInt8>) -> CGError
    private typealias GetProcessForPIDFn = @convention(c) (pid_t, UnsafeMutablePointer<ProcessSerialNumber>) -> OSStatus

    private typealias MainConnectionIDFn = @convention(c) () -> Int32
    private typealias GetActiveSpaceFn = @convention(c) (Int32) -> UInt64
    private typealias CopySpacesForWindowsFn = @convention(c) (Int32, Int32, CFArray) -> Unmanaged<CFArray>?

    private static let userGenerated: UInt32 = 0x200
    private static let allSpacesMask: Int32 = 0x7

    nonisolated(unsafe) private static let handle = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY)

    private static func symbol<T>(_ name: String, in lib: UnsafeMutableRawPointer?, as _: T.Type) -> T? {
        guard let ptr = dlsym(lib, name) else { return nil }
        return unsafeBitCast(ptr, to: T.self)
    }

    static func focusWindow(_ windowID: CGWindowID, pid: pid_t) -> Bool {
        guard let setFront = symbol("_SLPSSetFrontProcessWithOptions", in: handle, as: SetFrontProcessFn.self),
              let postEvent = symbol("SLPSPostEventRecordTo", in: handle, as: PostEventRecordFn.self),
              let getPSN = symbol("GetProcessForPID", in: UnsafeMutableRawPointer(bitPattern: -2), as: GetProcessForPIDFn.self)
        else { return false }

        var psn = ProcessSerialNumber()
        guard getPSN(pid, &psn) == noErr else { return false }
        guard setFront(&psn, windowID, userGenerated) == .success else { return false }

        var bytes = [UInt8](repeating: 0, count: 0xf8)
        bytes[0x04] = 0xf8
        bytes[0x3a] = 0x10
        withUnsafeBytes(of: windowID) { idBytes in
            for (i, b) in idBytes.enumerated() { bytes[0x3c + i] = b }
        }
        for i in 0x20..<0x30 { bytes[i] = 0xff }
        bytes[0x08] = 0x01
        _ = postEvent(&psn, &bytes)
        bytes[0x08] = 0x02
        _ = postEvent(&psn, &bytes)
        return true
    }

    static func spaces(of windowID: CGWindowID) -> [UInt64] {
        guard let connection = symbol("SLSMainConnectionID", in: handle, as: MainConnectionIDFn.self),
              let copySpaces = symbol("SLSCopySpacesForWindows", in: handle, as: CopySpacesForWindowsFn.self),
              let spaces = copySpaces(connection(), allSpacesMask, [windowID] as CFArray)?.takeRetainedValue()
        else { return [] }
        return (spaces as? [UInt64]) ?? []
    }

    static func activeSpace() -> UInt64? {
        guard let connection = symbol("SLSMainConnectionID", in: handle, as: MainConnectionIDFn.self),
              let getActive = symbol("SLSGetActiveSpace", in: handle, as: GetActiveSpaceFn.self)
        else { return nil }
        return getActive(connection())
    }
}
