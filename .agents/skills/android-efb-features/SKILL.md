---
name: android-efb-features
description: Workflow for implementing aviation EFB features (GPS location tracking, PDF plate viewer with night mode, MapLibre map integration) on Android.
---

# Android EFB Feature Implementation Guide

When building or extending aviation EFB features in the Kotlin + Jetpack Compose Android client:

## 1. Own-Ship Location & Tracking
- **MapLibre LocationComponent**: Activate `mapLibreMap.locationComponent` using `LocationComponentActivationOptions` with a translucent blue accuracy circle (`#3B82F6`).
- **Runtime Permissions**: Use `rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions())` for `ACCESS_FINE_LOCATION` and `ACCESS_COARSE_LOCATION`.
- **Camera Modes**: Cycle camera tracking through:
  - `LocationTrackingMode.NONE` (Free Pan)
  - `LocationTrackingMode.TRACKING` (Position Centered, North Up)
  - `LocationTrackingMode.TRACKING_COMPASS` (Position Centered, Heading Up)
- **User Pan Reset**: Listen via `OnCameraTrackingChangedListener` (`onCameraTrackingDismissed`) to reset state to `NONE` when the user manually drags the map.

## 2. Native PDF Instrument Plate Viewer
- **Native Rendering**: Use `android.graphics.pdf.PdfRenderer` via `ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)` for high-DPI rendering without external heavy dependencies.
- **Local Disk Cache**: Cache fetched PDFs in `context.cacheDir/plates/` by URL hash for full offline cockpit capability.
- **Download Fallback**: Attempt direct PDF URL stream first, with fallback to `/dtpp/plate?url=...` server proxy if direct fetch is blocked by CORS/network.
- **Cockpit Night Mode**: Apply a `ColorMatrix` inversion filter `ColorMatrix(floatArrayOf(-1f, 0f, 0f, 0f, 255f, 0f, -1f, 0f, 0f, 255f, 0f, 0f, -1f, 0f, 255f, 0f, 0f, 0f, 1f, 0f))` on `Image` to invert white plate backgrounds for night flying safety.
