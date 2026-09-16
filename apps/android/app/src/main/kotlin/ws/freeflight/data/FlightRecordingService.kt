package ws.freeflight.data

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import ws.freeflight.MainActivity
import ws.freeflight.container

class FlightRecordingService : Service(), LocationListener {

    private var locationManager: LocationManager? = null
    private val serviceScope = CoroutineScope(Dispatchers.Main + Job())
    private var notificationJob: Job? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action
        if (action == ACTION_STOP) {
            stopRecordingAndSelf()
            return START_NOT_STICKY
        }

        val notification = buildNotification("Flight recording started...")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(
                NOTIFICATION_ID,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION
            )
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }

        container.flightRecording.startRecording()
        startLocationUpdates()
        startNotificationUpdates()

        return START_STICKY
    }

    private fun startLocationUpdates() {
        locationManager = getSystemService(Context.LOCATION_SERVICE) as? LocationManager
        try {
            val provider = if (locationManager?.isProviderEnabled(LocationManager.GPS_PROVIDER) == true) {
                LocationManager.GPS_PROVIDER
            } else {
                LocationManager.NETWORK_PROVIDER
            }
            locationManager?.requestLocationUpdates(
                provider,
                1000L,
                2.0f,
                this,
            )
        } catch (e: SecurityException) {
            // Permission missing
            stopSelf()
        }
    }

    override fun onLocationChanged(location: Location) {
        val altFt = if (location.hasAltitude()) location.altitude * 3.28084 else 0.0
        val speedKt = if (location.hasSpeed()) location.speed * 1.94384 else null
        container.flightRecording.addPoint(
            lat = location.latitude,
            lon = location.longitude,
            altFt = altFt,
            timestampMs = location.time,
            speedKt = speedKt,
        )
    }

    override fun onProviderEnabled(provider: String) {}
    override fun onProviderDisabled(provider: String) {}

    private fun startNotificationUpdates() {
        notificationJob?.cancel()
        notificationJob = serviceScope.launch {
            while (isActive) {
                delay(1000L)
                val elapsed = container.flightRecording.elapsedSeconds.value
                val hours = elapsed / 3600
                val minutes = (elapsed % 3600) / 60
                val seconds = elapsed % 60
                val timeStr = String.format("%02d:%02d:%02d", hours, minutes, seconds)

                val altFt = container.flightRecording.latestPoint.value?.alt_ft ?: 0.0
                val speedKt = container.flightRecording.latestSpeedKt.value ?: 0.0
                val pointsCount = container.flightRecording.activePoints.value.size

                val content = "$timeStr | Alt: ${altFt.toInt()} ft | GS: ${speedKt.toInt()} kt | $pointsCount pts"
                val notification = buildNotification(content)
                val manager = getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
                manager?.notify(NOTIFICATION_ID, notification)
            }
        }
    }

    private fun buildNotification(contentText: String): Notification {
        val mainIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val mainPendingIntent = PendingIntent.getActivity(
            this,
            0,
            mainIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        val stopIntent = Intent(this, FlightRecordingService::class.java).apply {
            action = ACTION_STOP
        }
        val stopPendingIntent = PendingIntent.getService(
            this,
            1,
            stopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_menu_mylocation)
            .setContentTitle("Flight Recording Active")
            .setContentText(contentText)
            .setContentIntent(mainPendingIntent)
            .setOngoing(true)
            .addAction(android.R.drawable.ic_media_pause, "Stop", stopPendingIntent)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .build()
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Flight Recording",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Status of in-flight GPS track recording"
            setShowBadge(false)
        }
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
        manager?.createNotificationChannel(channel)
    }

    private fun stopRecordingAndSelf() {
        container.flightRecording.stopRecording()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        serviceScope.cancel()
        locationManager?.removeUpdates(this)
        super.onDestroy()
    }

    companion object {
        const val CHANNEL_ID = "flight_recording_channel"
        const val NOTIFICATION_ID = 2001
        const val ACTION_START = "ws.freeflight.action.START_RECORDING"
        const val ACTION_STOP = "ws.freeflight.action.STOP_RECORDING"

        fun start(context: Context) {
            val intent = Intent(context, FlightRecordingService::class.java).apply {
                action = ACTION_START
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }

        fun stop(context: Context) {
            val intent = Intent(context, FlightRecordingService::class.java).apply {
                action = ACTION_STOP
            }
            context.startService(intent)
        }
    }
}
