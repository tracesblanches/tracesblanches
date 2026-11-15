package fr.skimap.app.ui.map

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import androidx.core.graphics.createBitmap
import androidx.core.graphics.toColorInt
import fr.skimap.core.MapSymbol
import fr.skimap.core.SymbolShape
import kotlin.math.roundToInt

/**
 * Draws the symbols of the Rust core ([MapSymbol]: shape, colours, size) into bitmaps,
 * used both by the map style (`icon-image`) and by the legend, so they look the same.
 */
object SymbolBitmaps {
    private const val OUTLINE = 0.14f
    private const val DOUBLE_WIDTH = 1.75f

    /** Bitmap of [symbol] at [density] (pixels per dp), its density set for MapLibre. */
    fun draw(
        symbol: MapSymbol,
        density: Float,
        densityDpi: Int,
    ): Bitmap {
        val s = symbol.sizeDp.toFloat() * density
        val w = if (symbol.shape == SymbolShape.DOUBLE_DIAMOND) s * DOUBLE_WIDTH else s
        val bmp = createBitmap(w.roundToInt().coerceAtLeast(1), s.roundToInt().coerceAtLeast(1))
        bmp.density = densityDpi
        shape(Canvas(bmp), symbol, s, w)
        return bmp
    }

    private fun shape(
        c: Canvas,
        symbol: MapSymbol,
        s: Float,
        w: Float,
    ) {
        val fill = paint(symbol.fill, Paint.Style.FILL)
        val stroke = paint(symbol.stroke, Paint.Style.STROKE).apply { strokeWidth = s * OUTLINE }
        val inset = s * OUTLINE / 2f
        when (symbol.shape) {
            SymbolShape.CIRCLE -> {
                disc(c, s, inset, fill, stroke)
            }

            SymbolShape.SQUARE -> {
                val r = RectF(inset * 2, inset * 2, s - inset * 2, s - inset * 2)
                c.drawRect(r, fill)
                c.drawRect(r, stroke)
            }

            SymbolShape.DIAMOND -> {
                diamond(c, 0f, s, inset, fill, stroke)
            }

            SymbolShape.DOUBLE_DIAMOND -> {
                diamond(c, 0f, s, inset, fill, stroke)
                diamond(c, w - s, s, inset, fill, stroke)
            }

            SymbolShape.RING -> {
                // Coloured ring on a wider casing ring.
                val radius = s / 2 - inset * 3
                val casing = paint(symbol.stroke, Paint.Style.STROKE).apply { strokeWidth = s * OUTLINE * 3 }
                val ring = paint(symbol.fill, Paint.Style.STROKE).apply { strokeWidth = s * OUTLINE * 2 }
                c.drawCircle(s / 2, s / 2, radius, casing)
                c.drawCircle(s / 2, s / 2, radius, ring)
            }

            SymbolShape.ARROW -> {
                arrow(c, s, fill, stroke)
            }

            SymbolShape.CROSS, SymbolShape.CHECK, SymbolShape.DROP -> {
                disc(c, s, inset, fill, stroke)
                glyph(c, symbol, s)
            }

            SymbolShape.HOUSE -> {
                house(c, s, inset, symbol, fill, stroke)
            }
        }
    }

    /** White glyph of the disc symbols: cross, tick or water drop. */
    private fun glyph(
        c: Canvas,
        symbol: MapSymbol,
        s: Float,
    ) {
        when (symbol.shape) {
            SymbolShape.CROSS -> {
                val g = glyphPaint(symbol.stroke, s)
                val a = s * 0.32f
                c.drawLine(a, a, s - a, s - a, g)
                c.drawLine(s - a, a, a, s - a, g)
            }

            SymbolShape.CHECK -> {
                val p =
                    Path().apply {
                        moveTo(s * 0.28f, s * 0.52f)
                        lineTo(s * 0.44f, s * 0.68f)
                        lineTo(s * 0.73f, s * 0.34f)
                    }
                c.drawPath(p, glyphPaint(symbol.stroke, s))
            }

            else -> {
                val p =
                    Path().apply {
                        moveTo(s * 0.5f, s * 0.2f)
                        cubicTo(s * 0.5f, s * 0.2f, s * 0.72f, s * 0.48f, s * 0.72f, s * 0.6f)
                        cubicTo(s * 0.72f, s * 0.73f, s * 0.62f, s * 0.82f, s * 0.5f, s * 0.82f)
                        cubicTo(s * 0.38f, s * 0.82f, s * 0.28f, s * 0.73f, s * 0.28f, s * 0.6f)
                        cubicTo(s * 0.28f, s * 0.48f, s * 0.5f, s * 0.2f, s * 0.5f, s * 0.2f)
                        close()
                    }
                c.drawPath(p, paint(symbol.stroke, Paint.Style.FILL))
            }
        }
    }

    private fun paint(
        hex: String,
        style: Paint.Style,
    ) = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = hex.toColorInt()
        this.style = style
        strokeJoin = Paint.Join.ROUND
        strokeCap = Paint.Cap.ROUND
    }

    private fun glyphPaint(
        hex: String,
        s: Float,
    ) = paint(hex, Paint.Style.STROKE).apply { strokeWidth = s * 0.13f }

    private fun disc(
        c: Canvas,
        s: Float,
        inset: Float,
        fill: Paint,
        stroke: Paint,
    ) {
        c.drawCircle(s / 2, s / 2, s / 2 - inset, fill)
        c.drawCircle(s / 2, s / 2, s / 2 - inset, stroke)
    }

    private fun diamond(
        c: Canvas,
        x: Float,
        s: Float,
        inset: Float,
        fill: Paint,
        stroke: Paint,
    ) {
        val p =
            Path().apply {
                moveTo(x + s / 2, inset)
                lineTo(x + s - inset, s / 2)
                lineTo(x + s / 2, s - inset)
                lineTo(x + inset, s / 2)
                close()
            }
        c.drawPath(p, fill)
        c.drawPath(p, stroke)
    }

    private fun arrow(
        c: Canvas,
        s: Float,
        fill: Paint,
        stroke: Paint,
    ) {
        val p =
            Path().apply {
                moveTo(s * 0.12f, s * 0.36f)
                lineTo(s * 0.52f, s * 0.36f)
                lineTo(s * 0.52f, s * 0.16f)
                lineTo(s * 0.90f, s * 0.5f)
                lineTo(s * 0.52f, s * 0.84f)
                lineTo(s * 0.52f, s * 0.64f)
                lineTo(s * 0.12f, s * 0.64f)
                close()
            }
        c.drawPath(p, stroke)
        c.drawPath(p, fill)
    }

    private fun house(
        c: Canvas,
        s: Float,
        inset: Float,
        symbol: MapSymbol,
        fill: Paint,
        stroke: Paint,
    ) {
        val r = RectF(inset, inset, s - inset, s - inset)
        c.drawRoundRect(r, s * 0.18f, s * 0.18f, fill)
        c.drawRoundRect(r, s * 0.18f, s * 0.18f, stroke)
        val glyph = paint(symbol.stroke, Paint.Style.FILL)
        val p =
            Path().apply {
                moveTo(s * 0.5f, s * 0.2f)
                lineTo(s * 0.8f, s * 0.48f)
                lineTo(s * 0.71f, s * 0.48f)
                lineTo(s * 0.71f, s * 0.78f)
                lineTo(s * 0.29f, s * 0.78f)
                lineTo(s * 0.29f, s * 0.48f)
                lineTo(s * 0.2f, s * 0.48f)
                close()
            }
        c.drawPath(p, glyph)
    }
}
