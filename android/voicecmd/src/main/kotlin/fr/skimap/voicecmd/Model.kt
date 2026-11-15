package fr.skimap.voicecmd

// What the module exchanges with the app. The classifier is the core's (Rust,
// crates/intent through crates/ffi/src/voicecmd.rs); the app adapts it to
// CommandClassifier, so this module knows nothing of the bindings.

/** The closed list of commands (crates/intent `Intent`). */
enum class Command {
    WHERE_AM_I,
    TIME_LEFT,
    NEXT_DIRECTION,
    BRING_ME_BACK,
    TURNAROUND_TIME,
    REPORT_TRACKS_SET,
    REPORT_GROOMED,
    BATTERY,
    REPEAT,
}

/** Language of the question and of the replies. */
enum class CommandLang { FR, EN }

/** What was heard and understood: [command] null when not understood. */
data class Heard(
    val text: String,
    val command: Command?,
    val lang: CommandLang,
)

/** Transcription → command (the core's classifier, no language model). */
fun interface CommandClassifier {
    fun classify(
        text: String,
        defaultLang: CommandLang,
    ): Heard
}

/** Does what a command asks (says the answer through the guidance voice). */
fun interface CommandHandler {
    fun handle(heard: Heard)
}
