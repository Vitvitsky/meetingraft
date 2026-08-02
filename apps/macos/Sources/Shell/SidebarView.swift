import SwiftUI

/// Боковая навигация приложения.
struct SidebarView: View {
    @Binding var selection: AppDestination?

    var body: some View {
        List(AppDestination.allCases, selection: $selection) { destination in
            Label(destination.title, systemImage: destination.systemImage)
                .tag(destination)
        }
        .navigationTitle("MeetingRaft")
    }
}
