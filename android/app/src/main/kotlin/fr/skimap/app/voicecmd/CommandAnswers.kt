package fr.skimap.app.voicecmd

import fr.skimap.app.guidance.RustTrackingCore
import fr.skimap.core.ReportKind
import fr.skimap.core.ReportOutcome
import fr.skimap.core.VoiceAnswers
import fr.skimap.core.VoiceCommand
import fr.skimap.core.classifyCommand
import fr.skimap.core.commandQuestions
import fr.skimap.tracking.Batch
import fr.skimap.tracking.Utterance
import fr.skimap.voicecmd.Command
import fr.skimap.voicecmd.CommandClassifier
import fr.skimap.voicecmd.CommandLang
import fr.skimap.voicecmd.Heard
import fr.skimap.core.CommandLang as CoreLang

/** The core's classifier (crates/intent, no language model) behind the module's interface. */
object RustCommandClassifier : CommandClassifier {
    /** The questions of the closed list, both languages, without punctuation: the recognizer's hotwords. */
    val hotwords: List<String> by lazy {
        (commandQuestions(CoreLang.FR) + commandQuestions(CoreLang.EN)).map { it.replace(PUNCTUATION, "").trim() }
    }

    private val PUNCTUATION = Regex("[?.:!]")

    override fun classify(
        text: String,
        defaultLang: CommandLang,
    ): Heard {
        val h = classifyCommand(text, if (defaultLang == CommandLang.FR) CoreLang.FR else CoreLang.EN)
        return Heard(h.text, h.command?.let(::command), if (h.lang == CoreLang.FR) CommandLang.FR else CommandLang.EN)
    }

    fun command(c: VoiceCommand): Command =
        when (c) {
            VoiceCommand.WHERE_AM_I -> Command.WHERE_AM_I
            VoiceCommand.TIME_LEFT -> Command.TIME_LEFT
            VoiceCommand.NEXT_DIRECTION -> Command.NEXT_DIRECTION
            VoiceCommand.BRING_ME_BACK -> Command.BRING_ME_BACK
            VoiceCommand.TURNAROUND_TIME -> Command.TURNAROUND_TIME
            VoiceCommand.REPORT_TRACKS_SET -> Command.REPORT_TRACKS_SET
            VoiceCommand.REPORT_GROOMED -> Command.REPORT_GROOMED
            VoiceCommand.BATTERY -> Command.BATTERY
            VoiceCommand.REPEAT -> Command.REPEAT
        }
}

/**
 * Sends a community report (the community module, when the app has one: not yet, the
 * answer says so). True when recorded (sent, or kept until there is network).
 */
fun interface Reports {
    fun report(
        kind: ReportKind,
        tMs: Long,
    ): Boolean
}

/** The outing followed, seen from a command (on the location service's thread). */
class Outing(
    val core: RustTrackingCore,
    val planDir: String,
    /** The last thing said (for « Répète »). */
    val lastSaid: Utterance?,
)

/** What a command does: what to say, and the core's batch when it changed the route (« ramène-moi »). */
data class Answer(
    val said: List<Utterance>,
    val batch: Batch? = null,
)

/**
 * The answer of each command (A11): always a call of the core or a phrase of the voice
 * bank, never a text of its own; a command about the outing without an outing says so.
 */
class CommandAnswers(
    private val answers: VoiceAnswers,
    /** Battery level, percent, and the autonomy estimated, seconds (null: unknown). */
    private val battery: () -> Pair<Double, Double?>,
    private val reports: Reports? = null,
) {
    fun answer(
        heard: Heard,
        outing: Outing?,
        tMs: Long,
    ): Answer {
        val lang = if (heard.lang == CommandLang.FR) CoreLang.FR else CoreLang.EN
        val say = { s: fr.skimap.core.SpokenView -> Answer(listOf(RustTrackingCore.utterance(s))) }
        val c = heard.command ?: return say(answers.notUnderstood(lang, tMs))
        return when (c) {
            Command.BATTERY -> {
                battery().let { (p, a) -> say(answers.battery(p, a, tMs)) }
            }

            Command.REPORT_TRACKS_SET, Command.REPORT_GROOMED -> {
                val kind = if (c == Command.REPORT_GROOMED) ReportKind.GROOMED else ReportKind.TRACKS_SET
                val ok = reports?.report(kind, tMs) == true
                say(answers.report(kind, if (ok) ReportOutcome.RECORDED else ReportOutcome.UNAVAILABLE, lang, tMs))
            }

            Command.REPEAT -> {
                outing?.lastSaid?.let { Answer(listOf(it)) } ?: say(answers.nothingToRepeat(lang, tMs))
            }

            else -> {
                if (outing == null) say(answers.noOuting(lang, tMs)) else onOuting(c, outing, lang, tMs)
            }
        }
    }

    private fun onOuting(
        c: Command,
        o: Outing,
        lang: CoreLang,
        tMs: Long,
    ): Answer {
        val core = o.core
        val one = { u: Utterance? -> Answer(listOfNotNull(u)) }
        return when (c) {
            Command.WHERE_AM_I -> {
                Answer(listOf(RustTrackingCore.utterance(answers.whereAmI(core.session.guidance(), tMs))))
            }

            Command.TIME_LEFT -> {
                one(core.sayRemaining(tMs))
            }

            Command.NEXT_DIRECTION -> {
                one(core.sayNext(tMs))
            }

            Command.TURNAROUND_TIME -> {
                Answer(listOf(RustTrackingCore.utterance(answers.turnaround(o.planDir, tMs, lang))))
            }

            // Already going back: where it stands.
            Command.BRING_ME_BACK -> {
                if (core.guidance().route == "back") one(core.sayRemaining(tMs)) else Answer(emptyList(), core.backStart(tMs))
            }

            else -> {
                Answer(emptyList())
            }
        }
    }
}
