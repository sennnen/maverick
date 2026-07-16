import SwiftUI
import UIKit

/// A three-wheel countdown picker (hours · minutes · seconds) that scrolls endlessly
/// in either direction — no end-stops, so 59 → 0 → 59 wraps like the iOS Clock app's
/// timer. SwiftUI's `Picker(.wheel)` hard-stops at the ends; UIKit's `UIPickerView`
/// has no native looping either, so we fake it: each component is backed by a very
/// large virtual row count (real values repeated `loops` times) and we keep the
/// selection parked in the middle band, giving effectively-infinite runway both ways.
/// One picker with three components → a single selection bar spanning all three, the
/// authentic Clock look.
struct LoopingTimePicker: UIViewRepresentable {
    @Binding var hours: Int
    @Binding var minutes: Int
    @Binding var seconds: Int

    /// Real value counts per component: hours 0…23, minutes/seconds 0…59.
    private static let counts = [24, 60, 60]
    private static let units = ["h", "m", "s"]
    /// How many times the real values repeat. 1000 × 60 = 60 000 rows — trivial memory
    /// (titles are lazy) and far more spins than anyone will do in a sitting.
    private static let loops = 1000

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> UIPickerView {
        let picker = UIPickerView()
        picker.dataSource = context.coordinator
        picker.delegate = context.coordinator
        context.coordinator.picker = picker
        // Park each wheel on its bound value in the middle band once laid out.
        DispatchQueue.main.async { context.coordinator.centerAll(animated: false) }
        return picker
    }

    func updateUIView(_ picker: UIPickerView, context: Context) {
        context.coordinator.parent = self
        // Re-center only the wheels whose external binding changed (e.g. a quick-chip
        // tap) — never fight the user's in-progress spin.
        context.coordinator.syncIfNeeded()
    }

    final class Coordinator: NSObject, UIPickerViewDataSource, UIPickerViewDelegate {
        var parent: LoopingTimePicker
        weak var picker: UIPickerView?
        init(_ parent: LoopingTimePicker) { self.parent = parent }

        private func real(_ c: Int) -> Int { LoopingTimePicker.counts[c] }
        private func virtual(_ c: Int) -> Int { real(c) * LoopingTimePicker.loops }
        private func bound(_ c: Int) -> Int {
            switch c { case 0: parent.hours; case 1: parent.minutes; default: parent.seconds }
        }
        private func setBound(_ c: Int, _ v: Int) {
            switch c { case 0: parent.hours = v; case 1: parent.minutes = v; default: parent.seconds = v }
        }

        func numberOfComponents(in _: UIPickerView) -> Int { 3 }

        func pickerView(_: UIPickerView, numberOfRowsInComponent c: Int) -> Int { virtual(c) }

        func pickerView(_: UIPickerView, titleForRow row: Int, forComponent c: Int) -> String? {
            "\(row % real(c)) \(LoopingTimePicker.units[c])"
        }

        func pickerView(_: UIPickerView, widthForComponent _: Int) -> CGFloat {
            (picker?.bounds.width ?? 300) / 3
        }

        func pickerView(_: UIPickerView, didSelectRow row: Int, inComponent c: Int) {
            setBound(c, row % real(c))
            // If the spin drifts toward an edge of the virtual range, silently re-park in
            // the middle band on the same visible value so the runway never runs out.
            recenterIfNearEdge(component: c, row: row)
        }

        /// Jump every wheel to a middle-band row showing its bound value.
        func centerAll(animated: Bool) {
            guard let picker else { return }
            for c in 0..<3 {
                picker.selectRow(midRow(c, value: bound(c)), inComponent: c, animated: animated)
            }
        }

        /// Re-park any wheel whose visible value no longer matches its binding.
        func syncIfNeeded() {
            guard let picker else { return }
            for c in 0..<3 where picker.selectedRow(inComponent: c) % real(c) != bound(c) {
                picker.selectRow(midRow(c, value: bound(c)), inComponent: c, animated: true)
            }
        }

        private func midRow(_ c: Int, value: Int) -> Int {
            (LoopingTimePicker.loops / 2) * real(c) + max(0, min(real(c) - 1, value))
        }

        private func recenterIfNearEdge(component c: Int, row: Int) {
            guard let picker else { return }
            let n = real(c)
            let margin = n * 20
            if row < margin || row > virtual(c) - margin {
                // Same displayed value, back in the middle — no animation so it's invisible.
                picker.selectRow(midRow(c, value: row % n), inComponent: c, animated: false)
            }
        }
    }
}
