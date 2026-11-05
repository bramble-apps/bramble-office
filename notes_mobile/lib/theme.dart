// Notes' look on iPhone and iPad (iOS 26): the system's grouped backgrounds and labels, Notes'
// yellow accent, the type sizes, and Liquid Glass's tints. Light and dark follow the system
// (Settings can pin one).
import 'package:flutter/material.dart';

import 'ui/look.dart';

/// The desktop's colour themes (kn-ui COLOR_THEMES, shared by the suite): id, name, main colour,
/// accent. "Default" is Notes' yellow; another theme's main colour tints the app.
const colorThemes = [
  ('default', 'Notes Yellow', 0xFFCC00, 0xFFCC00),
  ('indigo', 'Indigo', 0x3F51B5, 0x00BFA5),
  ('ocean', 'Ocean', 0x0277BD, 0x26C6DA),
  ('forest', 'Forest', 0x2E7D32, 0xFFB300),
  ('sunset', 'Sunset', 0xEF6C00, 0xEC407A),
  ('rose', 'Rose', 0xC2185B, 0x7E57C2),
  ('lavender', 'Lavender', 0x7E57C2, 0x26A69A),
  ('crimson', 'Crimson', 0xC62828, 0xFFC107),
  ('teal', 'Teal', 0x00897B, 0xFF7043),
  ('sand', 'Sand', 0x8D6E63, 0x26A69A),
  ('slate', 'Slate', 0x546E7A, 0xFF8F00),
  ('midnight', 'Midnight', 0x1A237E, 0x00E5FF),
  ('graphite', 'Graphite', 0x616161, 0x9E9E9E),
];

class Palette {
  const Palette._(this.light);
  final bool light;

  /// Settings' colour theme and Glass (set at start and when Settings changes them).
  static String colorTheme = 'default';
  static bool glassOn = true;

  static const lightPalette = Palette._(true);
  static const darkPalette = Palette._(false);

  /// Grouped lists' backdrop (Folders, the notes list) and their cards.
  Color get grouped => light ? const Color(0xFFF2F2F7) : const Color(0xFF000000);
  Color get card => light ? const Color(0xFFFFFFFF) : const Color(0xFF1C1C1E);
  Color get cardPressed => light ? const Color(0xFFD1D1D6) : const Color(0xFF3A3A3C);

  /// The note's page.
  Color get page => light ? const Color(0xFFFFFFFF) : const Color(0xFF000000);

  /// The sidebar on iPad.
  Color get sidebar => light ? const Color(0xFFF2F2F7) : const Color(0xFF000000);

  Color get label => light ? const Color(0xFF000000) : const Color(0xFFFFFFFF);
  Color get secondary => light ? const Color(0x993C3C43) : const Color(0x99EBEBF5);
  Color get tertiary => light ? const Color(0x4D3C3C43) : const Color(0x4DEBEBF5);
  Color get separator => light ? const Color(0x4A3C3C43) : const Color(0x99545458);
  Color get fill => light ? const Color(0x1F787880) : const Color(0x3D787880);
  Color get searchFill => light ? const Color(0x1F767680) : const Color(0x3D767680);

  /// Notes' tint (or the colour theme's, lifted on dark backgrounds when it's too deep to show,
  /// as the desktop does).
  Color get accent {
    final t = colorThemes.where((t) => t.$1 == colorTheme && t.$1 != 'default').firstOrNull;
    if (t == null) return light ? const Color(0xFFE5A800) : const Color(0xFFFFCC00);
    final c = Color(0xFF000000 | t.$3);
    if (light) return c;
    final lum = 0.2126 * c.r * 255 + 0.7152 * c.g * 255 + 0.0722 * c.b * 255;
    if (lum >= 90) return c;
    final k = ((90 - lum) / 90 * 0.55).clamp(0.0, 0.5);
    return Color.from(alpha: 1, red: c.r + (1 - c.r) * k, green: c.g + (1 - c.g) * k, blue: c.b + (1 - c.b) * k);
  }

  Color get red => light ? const Color(0xFFFF3B30) : const Color(0xFFFF453A);
  Color get orange => light ? const Color(0xFFFF9500) : const Color(0xFFFF9F0A);
  Color get purple => light ? const Color(0xFFAF52DE) : const Color(0xFFBF5AF2);
  Color get blue => light ? const Color(0xFF007AFF) : const Color(0xFF0A84FF);
  Color get gray => light ? const Color(0xFF8E8E93) : const Color(0xFF8E8E93);

  /// Liquid Glass: the surface tint over the blurred backdrop, its rim and its shadow.
  Color get glass => !glassOn ? (light ? const Color(0xFFFFFFFF) : const Color(0xFF2C2C2E)) : (light ? const Color(0xB8FFFFFF) : const Color(0x99303033));
  Color get glassRim => light ? const Color(0x66FFFFFF) : const Color(0x33FFFFFF);
  Color get glassEdge => light ? const Color(0x14000000) : const Color(0x26FFFFFF);
  Color get glassShadow => light ? const Color(0x1F000000) : const Color(0x66000000);

  /// Sheets and menus.
  Color get sheet => light ? const Color(0xFFF2F2F7) : const Color(0xFF1C1C1E);
  Color get sheetCard => light ? const Color(0xFFFFFFFF) : const Color(0xFF2C2C2E);

  /// Notes' highlight colours 1…5 (purple, pink, orange, mint, blue): the text, and the wash behind it.
  Color highlightText(int i) => switch (i) {
    1 => light ? const Color(0xFF9B3FCE) : const Color(0xFFD08CF5),
    2 => light ? const Color(0xFFD4317A) : const Color(0xFFFF7FB4),
    3 => light ? const Color(0xFFD4700A) : const Color(0xFFFFB259),
    4 => light ? const Color(0xFF0E9F7A) : const Color(0xFF63E6BE),
    _ => light ? const Color(0xFF1C6FE0) : const Color(0xFF6CB2FF),
  };
  Color highlightWash(int i) => highlightText(i).withValues(alpha: light ? 0.16 : 0.26);

  /// The checklist's ticked circle.
  Color get check => accent;

  /// The menus ported from Letters.
  Look get look => Look({
    'light': light,
    'colors': {
      'text': label.toARGB32(),
      'textDim': secondary.toARGB32(),
      'separator': separator.toARGB32(),
      'panel': (light ? const Color(0xFFF9F9F9) : const Color(0xFF252527)).toARGB32(),
      'windowStroke': (light ? const Color(0xFFD8D8DC) : const Color(0xFF3A3A3C)).toARGB32(),
      'weakText': secondary.toARGB32(),
      'accent': accent.toARGB32(),
      'field': sheetCard.toARGB32(),
    },
  });

  static Palette of(BuildContext context) => Theme.of(context).brightness == Brightness.light ? lightPalette : darkPalette;
}

/// The interface's type (SF Pro as Sfera: Text below 20 points, Display from 20).
class Type {
  static TextStyle text(double size, Color color, {FontWeight weight = FontWeight.w400, double? height, double? spacing}) => TextStyle(
    fontFamily: size >= 20 ? 'SferaDisplay' : 'Sfera',
    fontSize: size,
    color: color,
    fontWeight: weight,
    height: height,
    letterSpacing: spacing ?? _tracking(size),
  );

  /// SF Pro's tracking at its sizes (points).
  static double _tracking(double s) => s >= 34
      ? 0.4
      : s >= 28
      ? 0.38
      : s >= 22
      ? -0.26
      : s >= 20
      ? -0.45
      : s >= 17
      ? -0.43
      : s >= 15
      ? -0.23
      : s >= 13
      ? -0.08
      : 0;

  static TextStyle largeTitle(Color c) => text(34, c, weight: FontWeight.w700);
  static TextStyle title(Color c) => text(17, c, weight: FontWeight.w600);
  static TextStyle body(Color c) => text(17, c);
  static TextStyle callout(Color c) => text(16, c);
  static TextStyle subhead(Color c) => text(15, c);
  static TextStyle footnote(Color c) => text(13, c);
  static TextStyle caption(Color c) => text(12, c);
}

ThemeData notesTheme(Brightness b) {
  final p = b == Brightness.light ? Palette.lightPalette : Palette.darkPalette;
  return ThemeData(
    useMaterial3: true,
    brightness: b,
    fontFamily: 'Sfera',
    splashFactory: NoSplash.splashFactory,
    highlightColor: Colors.transparent,
    scaffoldBackgroundColor: p.grouped,
    colorScheme: ColorScheme.fromSeed(seedColor: p.accent, brightness: b, primary: p.accent, surface: p.card),
    textSelectionTheme: TextSelectionThemeData(cursorColor: p.accent, selectionColor: p.accent.withValues(alpha: 0.32), selectionHandleColor: p.accent),
  );
}
