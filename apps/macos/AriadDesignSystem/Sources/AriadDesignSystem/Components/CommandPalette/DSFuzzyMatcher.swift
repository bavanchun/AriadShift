import Foundation

/// The result of matching a query against a candidate: a score (higher is better) and the character offsets of
/// the candidate that matched, for highlighting.
public struct DSFuzzyMatch: Equatable, Sendable {
    public let score: Int
    /// Offsets into `Array(candidate)` (characters, not UTF-16), ascending.
    public let matchedOffsets: [Int]
}

/// Subsequence fuzzy matching for the command palette. Case and diacritics are ignored. Consecutive characters,
/// word starts and an early first match score higher, and every skipped character costs a point, so "cmp" ranks
/// "Compress PDF" above "Crop margins of PDF".
public enum DSFuzzyMatcher {
    static let matchPoints = 1
    static let consecutiveBonus = 5
    static let wordStartBonus = 8
    static let firstCharacterBonus = 10
    static let maxLeadingPenalty = 3

    /// Matches `query` against `candidate`. Spaces in the query are ignored. An empty query matches everything
    /// with score 0 and no highlighted characters. Returns nil when the query is not a subsequence.
    public static func match(_ query: String, in candidate: String) -> DSFuzzyMatch? {
        let needle = fold(query).filter { !$0.isWhitespace }
        guard !needle.isEmpty else { return DSFuzzyMatch(score: 0, matchedOffsets: []) }
        let hay = fold(candidate)
        let original = Array(candidate)
        guard needle.count <= hay.count else { return nil }

        let n = needle.count
        let m = hay.count
        let unreachable = Int.min / 4
        // best[i][j]: best score for needle[0...i] with needle[i] matched at hay[j]; from[i][j]: hay index of needle[i-1].
        var best = Array(repeating: Array(repeating: unreachable, count: m), count: n)
        var from = Array(repeating: Array(repeating: -1, count: m), count: n)

        for j in 0..<m where hay[j] == needle[0] {
            best[0][j] = matchPoints + positionBonus(j, in: original) - min(j, maxLeadingPenalty)
        }
        if n > 1 {
            for i in 1..<n {
                // Running maximum of best[i-1][k] + k over k <= j-2, so a jump from k to j costs one point per
                // skipped character: best[i-1][k] - (j - k - 1) == (best[i-1][k] + k) - (j - 1).
                var runningBest = unreachable
                var runningIndex = -1
                for j in 1..<m {
                    let k = j - 2
                    if k >= 0, best[i - 1][k] > unreachable, best[i - 1][k] + k > runningBest {
                        runningBest = best[i - 1][k] + k
                        runningIndex = k
                    }
                    guard hay[j] == needle[i] else { continue }
                    let gain = matchPoints + positionBonus(j, in: original)
                    let consecutive = best[i - 1][j - 1] > unreachable ? best[i - 1][j - 1] + consecutiveBonus : unreachable
                    let jump = runningBest > unreachable ? runningBest - (j - 1) : unreachable
                    if consecutive > unreachable, consecutive >= jump {
                        best[i][j] = consecutive + gain
                        from[i][j] = j - 1
                    } else if jump > unreachable {
                        best[i][j] = jump + gain
                        from[i][j] = runningIndex
                    }
                }
            }
        }

        guard let end = (0..<m).max(by: { best[n - 1][$0] < best[n - 1][$1] }), best[n - 1][end] > unreachable else {
            return nil
        }
        var offsets = [Int](repeating: 0, count: n)
        var j = end
        for i in stride(from: n - 1, through: 0, by: -1) {
            offsets[i] = j
            j = from[i][j]
        }
        return DSFuzzyMatch(score: best[n - 1][end], matchedOffsets: offsets)
    }

    /// Folds one character at a time, so the result has exactly one entry per character of `text` and offsets
    /// line up with the candidate.
    static func fold(_ text: String) -> [Character] {
        text.map { character in
            let folded = String(character).folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
            return folded.count == 1 ? folded.first! : character
        }
    }

    /// Word starts: the first character, a character after a space or punctuation, or an uppercase letter after a
    /// lowercase one ("toWord").
    static func positionBonus(_ index: Int, in original: [Character]) -> Int {
        if index == 0 { return firstCharacterBonus + wordStartBonus }
        let previous = original[index - 1]
        let current = original[index]
        if !previous.isLetter && !previous.isNumber { return wordStartBonus }
        if previous.isLowercase && current.isUppercase { return wordStartBonus }
        return 0
    }
}
