package ai.eclosion.gateterm.android.appearance

import android.content.Context
import android.graphics.Typeface
import ai.eclosion.gateterm.android.term.FontAdvance

data class LoadedTermFont(val typeface: Typeface, val advanceEm: Double)

/**
 * 终端字体按需加载器：确保内存中同一时刻最多只保留一个内置字体实例。
 */
object TermFontLoader {
    private var activeKind: TermFontKind? = null
    private var activeTypeface: LoadedTermFont? = null

    @Synchronized
    fun load(context: Context, kind: TermFontKind): LoadedTermFont {
        if (activeKind == kind && activeTypeface != null) {
            return activeTypeface!!
        }
        // 立即释放前一个自定义字体引用，保证内存里同一时刻只保留一个字体实例
        activeTypeface = null
        activeKind = null

        val assetPath = kind.assetPath
        val typeface = if (assetPath == null) {
            LoadedTermFont(Typeface.MONOSPACE, Double.NaN)
        } else {
            runCatching {
                val assets = context.applicationContext.assets
                val typeface = Typeface.createFromAsset(assets, assetPath)
                val advance = runCatching { assets.open(assetPath).use(FontAdvance::readEm) }.getOrDefault(Double.NaN)
                LoadedTermFont(typeface, advance)
            }.getOrDefault(LoadedTermFont(Typeface.MONOSPACE, Double.NaN))
        }
        activeKind = kind
        activeTypeface = typeface
        return typeface
    }

    @Synchronized
    internal fun activeKindForTest(): TermFontKind? = activeKind
}
