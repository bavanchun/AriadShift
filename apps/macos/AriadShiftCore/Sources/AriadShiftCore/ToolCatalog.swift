import Foundation

/// How many input documents a tool takes. `EngineClient.toolRuntimeInfo()` reads it from the
/// registry; the document being edited is always the first input.
public struct InputArity: Equatable, Sendable {
    public let min: Int
    public let max: Int

    public init(min: Int, max: Int) {
        self.min = min
        self.max = max
    }

    public static let one = InputArity(min: 1, max: 1)
    public static let none = InputArity(min: 0, max: 0)

    /// Documents the user adds besides the open one.
    public var extraRange: ClosedRange<Int> { Swift.max(min - 1, 0)...Swift.max(max - 1, 0) }
}

/// Why a tool cannot be started from the Tools menu yet.
public enum UnavailableReason: String, Equatable, Sendable {
    /// Needs a later release: an unsupported parameter shape, an AI or editing tool,
    /// or a tool that takes no document.
    case later
    /// Works on other formats, not on the open PDF; it belongs to File > Import.
    case otherFormat
    /// The registry facts (arity, secret parameters) could not be read, so the tool stays
    /// off rather than guessing: a wrong guess would show a password in a plain text field.
    case infoUnavailable

    /// The branch of the `mac.tools.disabled` message that explains this reason.
    public var tooltipBranch: String {
        switch self {
        case .later: "later"
        case .otherFormat: "format"
        case .infoUnavailable: "info"
        }
    }
}

public enum ToolAvailability: Equatable, Sendable {
    case available
    case unavailable(UnavailableReason)
}

/// A registry tool prepared for the Tools menu.
public struct ToolEntry: Identifiable, Equatable, Sendable {
    public var id: String { info.id }
    public let info: ToolInfo
    public let title: String
    public let form: ParamForm
    public let arity: InputArity
    public let availability: ToolAvailability
    /// The plan this menu was built for, used only to label Pro tools.
    public let plan: EntitlementPlan

    /// The tool belongs to the Pro tier. It is labelled in the menu and the run sheet.
    public var isPro: Bool { info.tier != "free" }

    /// A Pro tool on a plan that does not grant Pro. It can still be opened; the host refuses
    /// the run with `tier_not_allowed`, which the run sheet shows as an upgrade message.
    public var isProLocked: Bool { isPro && !plan.grantsPro }

    public var isAvailable: Bool { availability == .available }
}

/// Tools of one registry group, in menu order.
public struct ToolGroup: Identifiable, Equatable, Sendable {
    public var id: String
    public let tools: [ToolEntry]
}

public enum ToolCatalog {
    /// Registry groups in menu order. Unknown groups follow alphabetically.
    public static let groupOrder = [
        "organize", "convert", "optimize", "security", "overlay", "ocr", "annotate",
        "forms", "sign", "redact", "compare", "edit", "ai", "automation",
    ]

    /// Groups whose tools wait for a later release: AI needs a model pack and editing arrives
    /// with the document editor.
    private static let laterGroups: Set<String> = ["ai", "edit"]

    /// Tools held back although their group ships. `pdf.sign_token` stays off because PKCS#11
    /// token signing returns `tool_missing` in production builds and needs the sign pack, so
    /// offering an upgrade for it would promise something that still fails.
    private static let laterTools: Set<String> = ["pdf.sign_token"]

    // Interim display names until the shared catalogue carries translated tool names.
    private static let acronyms = [
        "pdf": "PDF", "ocr": "OCR", "qr": "QR", "csv": "CSV", "cbz": "CBZ", "toc": "TOC",
        "pdfa": "PDF/A", "pdfua": "PDF/UA", "dpi": "DPI", "url": "URL", "ai": "AI",
    ]

    /// `pdf.delete_pages` -> "Delete Pages"; a tool of another family keeps its prefix
    /// (`image.ocr` -> "Image OCR").
    public static func title(for id: String) -> String {
        let parts = id.split(separator: ".", maxSplits: 1).map(String.init)
        let words = (parts.count == 2 ? parts[1] : id).split(separator: "_").map(String.init)
        let prefix = parts.count == 2 && parts[0] != "pdf" ? [parts[0]] : []
        return (prefix + words)
            .map { acronyms[$0] ?? $0.prefix(1).uppercased() + $0.dropFirst() }
            .joined(separator: " ")
    }

    /// - Parameter runtimeInfo: arity and secret parameters per tool id. A tool without an
    ///   entry is unavailable (`infoUnavailable`): nothing is guessed, so an empty dictionary
    ///   (the registry facts failed to load) keeps every tool that needs them disabled.
    /// - Parameter plan: labels Pro tools in the menu. It never decides whether a run is allowed.
    public static func build(
        tools: [ToolInfo], runtimeInfo: [String: ToolRuntimeInfo] = [:], plan: EntitlementPlan = .free
    ) -> [ToolGroup] {
        var byGroup: [String: [ToolEntry]] = [:]
        for info in tools {
            let known = runtimeInfo[info.id]
            let form = try? ParamForm(schemaJSON: info.paramsSchemaJson, secretParams: known?.secretParams ?? [])
            let arity = known?.arity ?? .one
            let entry = ToolEntry(
                info: info,
                title: title(for: info.id),
                form: form ?? ParamForm.unsupported,
                arity: arity,
                availability: availability(of: info, form: form, runtimeInfo: known),
                plan: plan
            )
            byGroup[info.group, default: []].append(entry)
        }
        let known = groupOrder.filter { byGroup[$0] != nil }
        let unknown = byGroup.keys.filter { !groupOrder.contains($0) }.sorted()
        return (known + unknown).map { group in
            ToolGroup(id: group, tools: byGroup[group, default: []].sorted { ($0.title, $0.id) < ($1.title, $1.id) })
        }
    }

    private static func availability(of info: ToolInfo, form: ParamForm?, runtimeInfo: ToolRuntimeInfo?) -> ToolAvailability {
        if laterGroups.contains(info.group) || laterTools.contains(info.id) {
            return .unavailable(.later)
        }
        guard let runtimeInfo else {
            return .unavailable(.infoUnavailable)
        }
        if runtimeInfo.arity == .none {
            return .unavailable(.later)
        }
        if !info.inputFormats.contains("pdf") {
            return .unavailable(.otherFormat)
        }
        guard let form, form.isSupported else {
            return .unavailable(.later)
        }
        return .available
    }
}

extension ParamForm {
    /// Stands in for a schema that could not be read; it blocks the tool.
    static let unsupported = ParamForm(fields: [], blocked: ["schema"])

    init(fields: [ParamField], blocked: [String]) {
        self.fields = fields
        self.blocked = blocked
    }
}
