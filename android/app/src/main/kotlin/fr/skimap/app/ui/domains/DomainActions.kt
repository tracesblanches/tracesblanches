package fr.skimap.app.ui.domains

/** What the picker can do besides searching. */
data class DomainActions(
    val onQuery: (String) -> Unit = {},
    val onNearest: () -> Unit = {},
    val onPick: (String) -> Unit = {},
    val onClose: () -> Unit = {},
    /** Download or update the pack of a server site; `true`: mobile data allowed. */
    val onDownload: (String, Boolean) -> Unit = { _, _ -> },
    val onCancel: (String) -> Unit = {},
    val onRemove: (String) -> Unit = {},
    /** Touch on the map of France at (lon, lat). */
    val onMapTouch: (Double, Double) -> Unit = { _, _ -> },
)
