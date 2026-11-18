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
    ///
    /// - Parameters:
    ///   - names: the file name each file ends up with (default: its own name). Must match
    ///     `files` in count.
    ///   - reservations: names that concurrent jobs hold. A name is reserved from the moment it is
    ///     chosen until its file is on disk, so jobs that share one instance never pick the same name.
    public static func place(
        _ files: [URL],
        into directory: URL,
        names: [String]? = nil,
        reservations: NameReservations? = nil,
        fileManager: FileManager = .default
    ) throws -> [URL] {
        try place(files, into: directory, names: names, reservations: reservations, fileManager: fileManager, renamer: .system)
    }

    static func place(
        _ files: [URL],
        into directory: URL,
        names: [String]?,
        reservations: NameReservations?,
        fileManager: FileManager,
        renamer: ExclusiveRename
    ) throws -> [URL] {
        precondition(names == nil || names?.count == files.count, "one name per file")
        var placed: [URL] = []
        for (index, file) in files.enumerated() {
            let desired = directory.appendingPathComponent(names?[index] ?? file.lastPathComponent)
            var attempts = 0
            while true {
                let target = reservations?.reserve(desired) ?? uniqueURL(for: desired)
                defer { reservations?.release(target) }
                do {
                    try moveExclusively(file, to: target, fileManager: fileManager, renamer: renamer)
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
    ///
    /// The name is claimed atomically in one of two ways: an exclusive rename, or, on a volume
    /// without one, an exclusive create of the name followed by a rename over that empty file
    /// (`renameOverClaimedName`). If the volume offers neither, it throws `PlacementError`.
    static func moveExclusively(
        _ source: URL,
        to target: URL,
        fileManager: FileManager = .default,
        renamer: ExclusiveRename = .system
    ) throws {
        switch renamer.attempt(source.path, target.path) {
        case 0:
            return
        case EEXIST:
            throw CocoaError(.fileWriteFileExists)
        case EXDEV:
            // Another volume: the file cannot be renamed, so copy it beside the target first.
            try copyExclusively(source, to: target, fileManager: fileManager, renamer: renamer)
        case ENOTSUP, EINVAL:
            do {
                try renameOverClaimedName(source, to: target)
            } catch where isCrossVolume(error) {
                // The first attempt could not tell that the source lives on another volume.
                try copyExclusively(source, to: target, fileManager: fileManager, renamer: renamer)
            }
        case let code:
            throw CocoaError(.fileWriteUnknown, userInfo: [NSUnderlyingErrorKey: POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)])
        }
    }

    /// Whether `error` is the `EXDEV` that `rename(2)` reports across volumes.
    private static func isCrossVolume(_ error: Error) -> Bool {
        ((error as NSError).userInfo[NSUnderlyingErrorKey] as? POSIXError)?.code == .EXDEV
    }

    /// Copies `source` next to `target` under a private name, renames that copy to `target`
    /// exclusively, then deletes `source`. A failure leaves `source` and no scratch file behind.
    static func copyExclusively(
        _ source: URL,
        to target: URL,
        fileManager: FileManager = .default,
        renamer: ExclusiveRename = .system
    ) throws {
        let scratch = target.deletingLastPathComponent().appendingPathComponent(".ashift-\(UUID().uuidString).part")
        do {
            try fileManager.copyItem(at: source, to: scratch)
            switch renamer.attempt(scratch.path, target.path) {
            case 0:
                break
            case EEXIST:
                throw CocoaError(.fileWriteFileExists)
            case ENOTSUP, EINVAL:
                try renameOverClaimedName(scratch, to: target)
            case let code:
                throw CocoaError(.fileWriteUnknown, userInfo: [NSUnderlyingErrorKey: POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)])
            }
        } catch {
            try? fileManager.removeItem(at: scratch)
            throw error
        }
        // The result is in place; the caller deletes the staging folder anyway.
        try? fileManager.removeItem(at: source)
    }

    /// For a volume without exclusive rename: `open(O_CREAT | O_EXCL)` is atomic on every volume
    /// and fails when anything, a dangling link included, holds the name. The empty file it creates
    /// keeps every other exclusive creator (this code, the engine's `create_new`) off the name,
    /// and `rename(2)` then replaces only that file with `source`. A writer that replaces files
    /// without creating them exclusively could still win in that instant; no primitive on such a
    /// volume prevents it.
    static func renameOverClaimedName(_ source: URL, to target: URL) throws {
        let descriptor = open(target.path, O_CREAT | O_EXCL | O_WRONLY | O_CLOEXEC, 0o600)
        guard descriptor >= 0 else { throw placementError(forCreateErrno: errno) }
        var claimed = stat()
        let knowsClaim = fstat(descriptor, &claimed) == 0
        close(descriptor)
        guard rename(source.path, target.path) == 0 else {
            let code = errno
            var current = stat()
            // Drop the placeholder, but only if it is still the file this call created.
            if knowsClaim, lstat(target.path, &current) == 0, current.st_dev == claimed.st_dev, current.st_ino == claimed.st_ino {
                unlink(target.path)
            }
            throw CocoaError(.fileWriteUnknown, userInfo: [NSUnderlyingErrorKey: POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)])
        }
    }

    /// The error for a failed exclusive create of an output name.
    static func placementError(forCreateErrno code: Int32) -> Error {
        switch code {
        case EEXIST:
            return CocoaError(.fileWriteFileExists)
        case ENOTSUP, EOPNOTSUPP, EINVAL, ENOSYS:
            return PlacementError.exclusivePlacementUnsupported
        default:
            return CocoaError(.fileWriteUnknown, userInfo: [NSUnderlyingErrorKey: POSIXError(POSIXErrorCode(rawValue: code) ?? .EIO)])
        }
    }
}

/// An output could not be placed without risking the replacement of a file.
public enum PlacementError: Error, Equatable, Sendable {
    /// The destination volume supports neither an exclusive rename nor an exclusive create.
    case exclusivePlacementUnsupported
}

/// The exclusive-rename syscall behind `OutputPlacer`, as a seam for tests that must reach the
/// paths a volume without `RENAME_EXCL` takes. `attempt` returns 0, or the `errno` of the failure.
struct ExclusiveRename: Sendable {
    let attempt: @Sendable (_ source: String, _ target: String) -> Int32

    static let system = ExclusiveRename { source, target in
        renamex_np(source, target, UInt32(RENAME_EXCL)) == 0 ? 0 : errno
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
