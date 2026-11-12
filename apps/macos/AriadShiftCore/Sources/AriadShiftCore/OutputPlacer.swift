import Foundation
import Synchronization

/// Puts tool and import results next to their source without ever replacing a file.
public enum OutputPlacer {
    /// Whether anything, a dangling symbolic link included, occupies `url`. `lstat` does not
    /// follow a final symbolic link, so a link to a missing file still counts as taken.
    static func isTaken(_ url: URL) -> Bool {
        var info = stat()
        return lstat(url.path, &info) == 0
    }

    /// `name.pdf` if free, otherwise `name 2.pdf`, `name 3.pdf` ... A name also counts as taken
    /// when `isReserved` says so (see `NameReservations`).
    public static func uniqueURL(for desired: URL, isReserved: (URL) -> Bool = { _ in false }) -> URL {
        func taken(_ url: URL) -> Bool { isTaken(url) || isReserved(url) }
        guard taken(desired) else { return desired }
        let directory = desired.deletingLastPathComponent()
        let stem = desired.deletingPathExtension().lastPathComponent
        let ext = desired.pathExtension
        var index = 2
        while true {
            let name = ext.isEmpty ? "\(stem) \(index)" : "\(stem) \(index).\(ext)"
            let candidate = directory.appendingPathComponent(name)
            if !taken(candidate) { return candidate }
            index += 1
        }
    }

    /// The folder for results: the user's default folder when it exists, otherwise the folder of the input.
    public static func destinationDirectory(
        for input: URL,
        customFolder: URL?,
        fileManager: FileManager = .default
    ) -> URL {
        var isDirectory: ObjCBool = false
        if let customFolder, fileManager.fileExists(atPath: customFolder.path, isDirectory: &isDirectory), isDirectory.boolValue {
            return customFolder
        }
        return input.deletingLastPathComponent()
    }

    /// The paths a tool reported, as files inside `staging`. A path outside it, or one that
    /// resolves outside it through a symbolic link, throws: moving it would relocate a file
    /// the tool never produced, an input for example.
    public static func stagedFiles(_ paths: [String], in staging: URL) throws -> [URL] {
        let root = staging.resolvingSymlinksInPath().standardizedFileURL.path
        let prefix = root.hasSuffix("/") ? root : root + "/"
        return try paths.map { path in
            let url = URL(fileURLWithPath: path)
            guard url.resolvingSymlinksInPath().standardizedFileURL.path.hasPrefix(prefix) else {
                throw EngineError.internal(message: "the tool reported an output outside its working folder: \(path)")
            }
            return url
        }
    }

    /// Moves `files` into `directory` under unique names and returns the final locations.
    ///
    /// Choosing a free name and taking it are one atomic step (`moveExclusively`), so a writer
    /// that takes the chosen name first makes the move fail and the next free number is tried.
    public static func place(_ files: [URL], into directory: URL, fileManager: FileManager = .default) throws -> [URL] {
        var placed: [URL] = []
        for file in files {
            let desired = directory.appendingPathComponent(file.lastPathComponent)
            var attempts = 0
            while true {
                let target = uniqueURL(for: desired)
                do {
                    try moveExclusively(file, to: target, fileManager: fileManager)
                    placed.append(target)
                    break
                } catch let error as CocoaError where error.code == .fileWriteFileExists && attempts < 100 {
                    attempts += 1
                }
            }
        }
        return placed
    }

    /// Moves `source` to `target`, failing with `fileWriteFileExists` instead of replacing
    /// anything that is there, even when another writer creates it at the same moment.
    static func moveExclusively(_ source: URL, to target: URL, fileManager: FileManager = .default) throws {
        if renamex_np(source.path, target.path, UInt32(RENAME_EXCL)) == 0 { return }
        switch errno {
        case EEXIST:
            throw CocoaError(.fileWriteFileExists)
        case EXDEV:
            // Another volume: the file cannot be renamed, so copy it beside the target first.
            try copyExclusively(source, to: target, fileManager: fileManager)
        case ENOTSUP, EINVAL:
            // A volume without exclusive rename: `moveItem` still refuses an existing target,
            // though without the guarantee against a concurrent writer.
            try fileManager.moveItem(at: source, to: target)
        default:
            throw CocoaError(.fileWriteUnknown, userInfo: [NSUnderlyingErrorKey: POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)])
        }
    }

    /// Copies `source` next to `target` under a private name, renames that copy to `target`
    /// exclusively, then deletes `source`.
    static func copyExclusively(_ source: URL, to target: URL, fileManager: FileManager = .default) throws {
        let scratch = target.deletingLastPathComponent().appendingPathComponent(".ashift-\(UUID().uuidString).part")
        try fileManager.copyItem(at: source, to: scratch)
        if renamex_np(scratch.path, target.path, UInt32(RENAME_EXCL)) != 0 {
            let code = errno
            try? fileManager.removeItem(at: scratch)
            if code == EEXIST { throw CocoaError(.fileWriteFileExists) }
            if code == ENOTSUP || code == EINVAL {
                try fileManager.copyItem(at: source, to: target)
            } else {
                throw CocoaError(.fileWriteUnknown, userInfo: [NSUnderlyingErrorKey: POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)])
            }
        }
        try fileManager.removeItem(at: source)
    }
}

/// Output names handed out to jobs that run at the same time. Two imports of `x.md` and
/// `x.html` both want `x.pdf`; the first to reserve it keeps it and the other gets `x 2.pdf`,
/// before either engine run has written anything.
public final class NameReservations: Sendable {
    private let reserved = Mutex<Set<String>>([])

    public init() {}

    private static func key(_ url: URL) -> String {
        // Case-insensitive on purpose: the default volume format is.
        url.standardizedFileURL.path.lowercased()
    }

    /// The first name from `desired` that is neither on disk nor reserved; it is now reserved.
    public func reserve(_ desired: URL) -> URL {
        reserved.withLock { names in
            let url = OutputPlacer.uniqueURL(for: desired) { names.contains(Self.key($0)) }
            names.insert(Self.key(url))
            return url
        }
    }

    /// Frees a name once its job ended: a written file is found on disk from then on.
    public func release(_ url: URL) {
        _ = reserved.withLock { $0.remove(Self.key(url)) }
    }
}
