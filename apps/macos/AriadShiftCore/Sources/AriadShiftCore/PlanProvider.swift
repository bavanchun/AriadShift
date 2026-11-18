import Foundation

/// The plan a run executes under. The values match `ariad_core::tools::Plan`; the Rust host
/// parses them and is the only place that decides which tools a plan may run.
public enum EntitlementPlan: String, Equatable, Sendable {
    case free, pro, team, enterprise

    /// Display hint only (badges, menu state). Whether a run is allowed is decided by the host.
    public var grantsPro: Bool { self != .free }
}

/// Where the app learns which plan it runs under.
public protocol PlanProvider: Sendable {
    /// - Throws: `EngineError.invalidRequest` when the source holds a value that is not a plan.
    func currentPlan() throws -> EntitlementPlan
}

/// The shipping provider. No purchase or licence system exists yet, so it is always Free
/// and ignores the process environment.
public struct ReleasePlanProvider: PlanProvider {
    public init() {}

    public func currentPlan() throws -> EntitlementPlan { .free }
}

/// The provider a client uses unless told otherwise. Debug builds read `ASHIFT_PLAN` so
/// tests and developers can run Pro tools; Release builds never see that code.
public func makeDefaultPlanProvider() -> any PlanProvider {
    #if DEBUG
    EnvironmentPlanProvider()
    #else
    ReleasePlanProvider()
    #endif
}

#if DEBUG
/// Reads `ASHIFT_PLAN` with the CLI's strict values (`free`, `pro`, `team`, `enterprise`).
/// Absent means Free; any other value is rejected. Compiled out of Release builds.
public struct EnvironmentPlanProvider: PlanProvider {
    private let environment: @Sendable () -> [String: String]

    public init(environment: @escaping @Sendable () -> [String: String] = { ProcessInfo.processInfo.environment }) {
        self.environment = environment
    }

    public func currentPlan() throws -> EntitlementPlan {
        guard let raw = environment()["ASHIFT_PLAN"] else { return .free }
        guard let plan = EntitlementPlan(rawValue: raw.trimmingCharacters(in: .whitespacesAndNewlines)) else {
            throw EngineError.invalidRequest(message: "ASHIFT_PLAN must be free, pro, team or enterprise")
        }
        return plan
    }
}
#endif
