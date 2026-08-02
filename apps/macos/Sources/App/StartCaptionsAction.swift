import SwiftUI

/// Focused action: старт demo captions из меню.
struct StartCaptionsKey: FocusedValueKey {
    typealias Value = () -> Void
}

extension FocusedValues {
    var startCaptions: (() -> Void)? {
        get { self[StartCaptionsKey.self] }
        set { self[StartCaptionsKey.self] = newValue }
    }
}
