import SwiftUI

/// Transactions for state changes that must never animate.
public enum DSTransactions {
    /// A bulk reload (a document replaced, a thumbnail cache swapped) changes many views at once. It runs with no
    /// animation and with `disablesAnimations`, so no `.animation(_:value:)` further up the tree turns the reload into
    /// motion.
    public static var bulkReload: Transaction {
        var transaction = Transaction(animation: nil)
        transaction.disablesAnimations = true
        return transaction
    }
}

/// Runs `body` as a bulk reload: the state it changes applies without any animation.
@MainActor
@discardableResult
public func withDSBulkReload<Result>(_ body: () throws -> Result) rethrows -> Result {
    try withTransaction(DSTransactions.bulkReload, body)
}

extension DSMotion {
    /// The animation of a thumbnail image arriving over its placeholder: the `fade` curve, or none under Reduce Motion,
    /// where the image simply appears. A screenful of thumbnails arrives while the user scrolls, so it is decoration
    /// that Reduce Motion drops rather than replaces.
    public static func thumbnailFade(reduceMotion: Bool) -> Animation? {
        reduceMotion ? nil : DSMotion.fade.animation(reduceMotion: false)
    }
}
