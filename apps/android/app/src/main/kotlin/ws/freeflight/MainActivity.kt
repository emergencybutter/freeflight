package ws.freeflight

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AirplanemodeActive
import androidx.compose.material.icons.filled.Map
import androidx.compose.material.icons.filled.Route
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.lifecycle.viewmodel.compose.viewModel
import ws.freeflight.ui.AircraftScreen
import ws.freeflight.ui.FreeflightTheme
import ws.freeflight.ui.FreeflightViewModel
import ws.freeflight.ui.MapScreen
import ws.freeflight.ui.PlanScreen
import ws.freeflight.ui.SettingsScreen

private enum class Tab(val label: String, val icon: ImageVector) {
    Map("Map", Icons.Default.Map),
    Plan("Plan", Icons.Default.Route),
    Aircraft("Aircraft", Icons.Default.AirplanemodeActive),
    Settings("Settings", Icons.Default.Settings),
}

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            FreeflightTheme {
                val viewModel: FreeflightViewModel = viewModel(
                    factory = FreeflightViewModel.Factory(container)
                )
                FreeflightScaffold(viewModel)
            }
        }
    }
}

@Composable
private fun FreeflightScaffold(viewModel: FreeflightViewModel) {
    var tab by rememberSaveable { mutableStateOf(Tab.Map) }

    Scaffold(
        bottomBar = {
            NavigationBar {
                Tab.entries.forEach { entry ->
                    NavigationBarItem(
                        selected = tab == entry,
                        onClick = { tab = entry },
                        icon = { Icon(entry.icon, contentDescription = entry.label) },
                        label = { Text(entry.label) },
                    )
                }
            }
        }
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            // The map stays composed whichever tab is showing, with the
            // other tabs drawn over it. Letting it leave the composition
            // would tear down MapLibre's GL surface and lose the camera —
            // so a pilot who checks the Plan tab mid-flight would come back
            // to a map that had forgotten where they were.
            MapScreen(viewModel, onOpenPlan = { tab = Tab.Plan })

            if (tab != Tab.Map) {
                Surface(Modifier.fillMaxSize()) {
                    when (tab) {
                        // Adding a waypoint means picking one off the map,
                        // so that button hands the map back rather than
                        // opening a second way to search for one.
                        Tab.Plan -> PlanScreen(
                            viewModel,
                            onAddWaypointClick = { tab = Tab.Map },
                        )
                        Tab.Aircraft -> AircraftScreen(viewModel)
                        Tab.Settings -> SettingsScreen(viewModel)
                        Tab.Map -> Unit
                    }
                }
            }
        }
    }
}
