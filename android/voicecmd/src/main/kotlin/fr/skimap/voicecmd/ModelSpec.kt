package fr.skimap.voicecmd

import java.io.File

/** One file of the speech model: where it comes from, its size and its sha256. */
data class ModelFile(
    val name: String,
    val url: String,
    val size: Long,
    val sha256: String,
)

/**
 * The speech model, an optional download (never in the APK): Parakeet-TDT-0.6B-v3 int8
 * (sherpa-onnx export, Hugging Face mirror of the sherpa-onnx author, revision pinned)
 * and Silero VAD v6.2.3. Sizes and digests checked on 2026-11-15 (the Hugging Face LFS
 * oids; tokens.txt identical to the one speak validated on the Pixel 8 Pro).
 */
data class ModelSpec(
    val id: String,
    val files: List<ModelFile>,
) {
    val totalBytes: Long get() = files.sumOf { it.size }

    companion object {
        private const val PARAKEET =
            "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/" +
                "resolve/ed5d745623472f97cfc2e6c2916ee248798fc5a8"
        private const val SILERO =
            "https://raw.githubusercontent.com/snakers4/silero-vad/v6.2.3/src/silero_vad/data/silero_vad.onnx"

        const val ENCODER = "encoder.int8.onnx"
        const val DECODER = "decoder.int8.onnx"
        const val JOINER = "joiner.int8.onnx"
        const val TOKENS = "tokens.txt"
        const val VAD = "silero_vad.onnx"
        const val TOKENIZER = "tokenizer.json"

        /** Made from [TOKENIZER] by [ModelInstaller]: the BPE pieces for the hotwords. */
        const val BPE_VOCAB = "bpe.vocab"

        /** 674 064 268 bytes in all (643 MiB). */
        val PARAKEET_V3_INT8 =
            ModelSpec(
                "parakeet-tdt-0.6b-v3-int8",
                listOf(
                    ModelFile(
                        ENCODER,
                        "$PARAKEET/$ENCODER",
                        652_282_294,
                        "1afbe69c1ffdabee94755f42e4daf1d93549131a3da11356bda84fcef0f2c479",
                    ),
                    ModelFile(
                        DECODER,
                        "$PARAKEET/$DECODER",
                        11_845_274,
                        "0222a86147299d4dee90edf95b6d13827edb42ad8b7c9cf0b2582f030937c0b5",
                    ),
                    ModelFile(
                        JOINER,
                        "$PARAKEET/$JOINER",
                        6_355_277,
                        "cbd02f3696e9944970111fb439d9c3fd365a80374565f13f2b9c7007d253fc98",
                    ),
                    ModelFile(
                        TOKENS,
                        "$PARAKEET/$TOKENS",
                        93_939,
                        "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
                    ),
                    // NVIDIA's tokenizer of the same model (revision pinned as in speak): the
                    // hotwords are encoded with Parakeet's own BPE.
                    ModelFile(
                        TOKENIZER,
                        "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3/resolve/541d1f99c6b0c3cd0b11a95167540bb8edefd82b/$TOKENIZER",
                        1_159_960,
                        "bd321b096832a3f270bd3b2a88823957920f1a5c5ada71114a26ea729d0cbe91",
                    ),
                    ModelFile(
                        VAD,
                        SILERO,
                        2_327_524,
                        "1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3",
                    ),
                ),
            )
    }
}

/**
 * The model on the phone: `<root>/<spec id>/<file>`, plus a marker written once every
 * file was downloaded and its sha256 checked ([ModelInstaller]).
 */
class ModelStore(
    root: File,
    val spec: ModelSpec = ModelSpec.PARAKEET_V3_INT8,
) {
    val dir: File = File(root, spec.id)

    fun file(name: String): File = File(dir, name)

    private val marker: File get() = File(dir, MARKER)

    /** Every file checked and in place (sizes re-checked: a file cut by the system is not installed). */
    val installed: Boolean
        get() = marker.isFile && spec.files.all { file(it.name).length() == it.size } && derivedOk()

    private fun derivedOk() = spec.files.none { it.name == ModelSpec.TOKENIZER } || file(ModelSpec.BPE_VOCAB).length() > 0

    /** Bytes on the storage (files and parts being downloaded). */
    val bytesOnDisk: Long get() = dir.listFiles()?.sumOf { it.length() } ?: 0L

    internal fun markInstalled() = marker.writeText(spec.files.joinToString("\n") { "${it.sha256}  ${it.name}" })

    /** Frees the space (the model is optional). */
    fun delete() {
        dir.deleteRecursively()
    }

    companion object {
        const val MARKER = "SHA256SUMS"
    }
}
