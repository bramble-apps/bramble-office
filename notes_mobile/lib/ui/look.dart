// The look the menus ported from Letters draw with (Letters' desktop/look.dart): here filled from
// Notes' palette (theme.dart), its interface font Sfera (SF Pro's metric match).
import 'package:flutter/widgets.dart';

class Look {
  Look(Map<String, dynamic> j)
    : light = j['light'] == true,
      glass = j['glass'] == true,
      translucent = j['translucent'] == true,
      classic = j['classic'] == true,
      toolbarBackground = j['toolbarBackground'] as String? ?? 'tinted',
      _c = (j['colors'] as Map<String, dynamic>?) ?? const {};
  final bool light;

  /// Settings > General: liquid glass buttons, the translucent inspector, the classic
  /// toolbar strip, what shows under the floating toolbar (tinted, feathered, solid).
  final bool glass, translucent, classic;
  final String toolbarBackground;
  final Map<String, dynamic> _c;

  Color _col(String k, int fallback) => Color((_c[k] as int?) ?? fallback);
  Color get text => _col('text', 0xFFE8E8EA);
  Color get textDim => _col('textDim', 0xFF9A9A9E);
  Color get separator => _col('separator', 0xFF3A3A3A);
  Color get panel => _col('panel', 0xFF232323);
  Color get sidebar => _col('sidebar', 0xFF222222);
  Color get control => _col('control', 0xFF3A3A3C);
  Color get accent => _col('accent', 0xFFF5A03C);
  Color get header => _col('header', 0xFF1E1E1E);
  Color get canvas => _col('canvas', 0xFF2B2B2B);
  Color get capsule => _col('capsule', 0xFF3C3C3C);
  Color get styleButton => _col('styleButton', 0xFF4C4C4C);
  Color get styleButtonText => _col('styleButtonText', 0xFFF0F0F0);

  /// A control's outline (light: a hairline, as macOS; dark: none).
  Color get controlStroke => _col('controlStroke', 0x00000000);

  /// egui's hovered widget fill (kn_ui theme: widgets.hovered.weak_bg_fill).
  Color get hover => _col('hover', light ? 0xFFE4E4E6 : 0xFF48484A);

  /// egui's window / popup outline.
  Color get windowStroke => _col('windowStroke', light ? 0xFFBEBEBE : 0xFF3C3C3C);

  /// egui's weak text (menu shortcuts); a disabled widget is drawn at half opacity.
  Color get weakText => _col('weakText', light ? 0xFF656565 : 0xFF8C8C8C);

  /// egui's text field fill and resting outline.
  Color get field => _col('field', light ? 0xFFFFFFFF : 0xFF1A1A1A);
  Color get fieldStroke => _col('fieldStroke', light ? 0xFFCCCCCC : 0xFF3C3C3C);

  /// liquid glass's surface tint; Pages' tint over the blur under the floating toolbar.
  Color get glassTint => _col('glassTint', light ? 0xFFFAFAFA : 0xFF2A2A2C);
  Color get toolbarTint => _col('toolbarTint', light ? 0x99F6F6F6 : 0x99272727);

  /// egui's window title bar (widgets.open.weak_bg_fill).
  Color get titleBar => _col('titleBar', light ? 0xFFDCDCDC : 0xFF2D2D2D);

  /// The document tab bar (tabs.rs bar): its track, the active tab, the lines between tabs.
  Color get tabBar => _col('tabBar', light ? 0xFFE8E8E8 : 0xFF353535);
  Color get tabActive => _col('tabActive', light ? 0xFFFFFFFF : 0xFF4A4A4A);
  Color get tabLine => _col('tabLine', light ? 0xFFC8C8C8 : 0xFF484848);

  /// theme::gray(v): a grey of the dark design, mapped for the appearance.
  Color gray(int v) {
    final g = _c['gray'] as Map<String, dynamic>?;
    final c = g?[v.toRadixString(16).padLeft(2, '0')] as int?;
    return Color(c ?? (0xFF000000 | v << 16 | v << 8 | v));
  }

  /// egui's FontId::proportional(size) / the "bold" family.
  static TextStyle font(double size, Color color, {bool bold = false}) => TextStyle(
    fontFamily: 'Sfera',
    fontSize: size,
    color: color,
    fontWeight: bold ? FontWeight.w700 : FontWeight.w400,
    height: 1.0,
    leadingDistribution: TextLeadingDistribution.even,
  );
}
