package fr.skimap.voicecmd

import org.json.JSONObject
import java.io.File
import java.io.IOException

/**
 * The BPE vocabulary sherpa-onnx needs to encode hotwords with Parakeet's own pieces
 * (`modeling_unit=bpe`), made from the model's `tokenizer.json` as speak does
 * (`app/scripts/fetch_models.sh`): the pieces in id order, which must be the order of
 * `tokens.txt` (checked), score minus the merge rank, special pieces 0.
 */
object BpeVocab {
    fun write(
        tokenizer: File,
        tokens: File,
        out: File,
    ) {
        val vocab = JSONObject(tokenizer.readText()).getJSONObject("model").getJSONObject("vocab")
        val ids = HashMap<String, Int>(vocab.length() * 2)
        for (k in vocab.keys()) ids[k] = vocab.getInt(k)
        // Every piece of the tokenizer has its id in tokens.txt (which adds the blank).
        val toks = HashMap<String, Int>(ids.size * 2)
        for (line in tokens.readLines()) {
            val sp = line.lastIndexOf(' ')
            if (sp > 0) toks[line.substring(0, sp)] = line.substring(sp + 1).toInt()
        }
        for ((piece, id) in ids) {
            if (toks[piece] != id) throw IOException("tokenizer.json and tokens.txt disagree on $piece")
        }
        val sorted = ids.entries.sortedBy { it.value }
        val text =
            buildString {
                for ((piece, id) in sorted) {
                    append(piece).append('\t').append(if (piece.startsWith("<")) "0.0" else "-$id.0").append('\n')
                }
            }
        val tmp = File(out.path + ".part")
        tmp.writeText(text)
        if (!tmp.renameTo(out)) throw IOException("$out: rename failed")
    }
}
