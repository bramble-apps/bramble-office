// Math Results (Notes' Math Notes): an expression typed with "=" at its end gets its answer,
// as an inline math token after the "=" (Insert Results). Variables set earlier in the note
// ("x = 5", "rent = 1200") can be used; + − × ÷ ^ %, parentheses, sqrt, sin, cos, tan, ln, log,
// π and e.
import 'dart:math' as m;

/// The answer to `expr` with `vars`, or null when it isn't a calculation.
double? evaluate(String expr, Map<String, double> vars) {
  final p = _Parser(expr.replaceAll('×', '*').replaceAll('÷', '/').replaceAll('−', '-').replaceAll(',', ''), vars);
  try {
    final v = p.expr();
    p.skip();
    if (p.i != p.s.length || !p.sawOperator) return null;
    return v.isFinite ? v : null;
  } catch (_) {
    return null;
  }
}

/// Variables the lines before set ("name = number" or "name = expression").
Map<String, double> variables(Iterable<String> lines) {
  final vars = <String, double>{};
  for (final l in lines) {
    final mt = RegExp(r'^\s*([A-Za-z_][A-Za-z_0-9]*)\s*=\s*([^=]+?)\s*$').firstMatch(l);
    if (mt == null) continue;
    final p = _Parser(mt[2]!, vars);
    try {
      final v = p.expr();
      p.skip();
      if (p.i == p.s.length) vars[mt[1]!] = v;
    } catch (_) {}
  }
  return vars;
}

/// A result as Notes shows it: up to 10 significant digits, no trailing zeros.
String formatResult(double v) {
  if (v == v.roundToDouble() && v.abs() < 1e15) return v.toInt().toString();
  var s = v.toStringAsPrecision(10);
  if (s.contains('e')) return s;
  if (s.contains('.')) s = s.replaceAll(RegExp(r'0+$'), '').replaceAll(RegExp(r'\.$'), '');
  return s;
}

class _Parser {
  _Parser(this.s, this.vars);
  final String s;
  final Map<String, double> vars;
  int i = 0;
  bool sawOperator = false;

  void skip() {
    while (i < s.length && s[i] == ' ') {
      i++;
    }
  }

  bool eat(String c) {
    skip();
    if (i < s.length && s[i] == c) {
      i++;
      return true;
    }
    return false;
  }

  double expr() {
    var v = term();
    while (true) {
      if (eat('+')) {
        sawOperator = true;
        v += term();
      } else if (eat('-')) {
        sawOperator = true;
        v -= term();
      } else {
        return v;
      }
    }
  }

  double term() {
    var v = power();
    while (true) {
      if (eat('*')) {
        sawOperator = true;
        v *= power();
      } else if (eat('/')) {
        sawOperator = true;
        v /= power();
      } else if (_adjacent()) {
        // side by side is a product: "(2)3", "2(3+4)", "(1+2)(3+4)", "2x", "3(x+1)", "2π"
        sawOperator = true;
        v *= power();
      } else {
        return v;
      }
    }
  }

  bool _adjacent() {
    skip();
    return i < s.length && RegExp(r'[(\d.A-Za-zπ_]').hasMatch(s[i]);
  }

  double power() {
    final b = unary();
    if (eat('^')) {
      sawOperator = true;
      return m.pow(b, power()).toDouble();
    }
    return b;
  }

  double unary() {
    if (eat('-')) return -unary();
    if (eat('+')) return unary();
    var v = atom();
    if (eat('%')) {
      sawOperator = true;
      v /= 100;
    }
    return v;
  }

  double atom() {
    skip();
    if (eat('(')) {
      final v = expr();
      if (!eat(')')) throw const FormatException();
      return v;
    }
    final num = RegExp(r'\d+(\.\d+)?|\.\d+').matchAsPrefix(s, i);
    if (num != null) {
      i = num.end;
      return double.parse(num[0]!);
    }
    final id = RegExp(r'[A-Za-zπ_][A-Za-z_0-9]*').matchAsPrefix(s, i);
    if (id != null) {
      i = id.end;
      final name = id[0]!;
      const fns = {'sqrt': m.sqrt, 'sin': m.sin, 'cos': m.cos, 'tan': m.tan, 'ln': m.log};
      if (fns.containsKey(name) || name == 'log') {
        sawOperator = true;
        final a = atom();
        return name == 'log' ? m.log(a) / m.ln10 : fns[name]!(a).toDouble();
      }
      if (name == 'π' || name == 'pi') return m.pi;
      if (name == 'e') return m.e;
      final v = vars[name];
      if (v == null) throw const FormatException();
      sawOperator = true;
      return v;
    }
    throw const FormatException();
  }
}
