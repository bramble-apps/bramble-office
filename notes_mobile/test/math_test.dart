import 'package:flutter_test/flutter_test.dart';
import 'package:notes/math.dart';

void main() {
  test('side by side is a product, as the desktop', () {
    const cases = {'2(5)*4': 40.0, '(2)(3)': 6.0, '(1+2)4': 12.0, '2(3+4)': 14.0, '2(5)': 10.0, '(2+3)*4': 20.0, '2π': 6.283185307179586};
    cases.forEach((e, v) => expect(evaluate(e, const {}), closeTo(v, 1e-9), reason: e));
    expect(evaluate('3(x+1)', const {'x': 2}), 9);
    expect(evaluate('2x', const {'x': 4}), 8);
    // the desktop's cases (nt-app editor/math.rs, e796c8b)
    const desktop = {'2(5)*4': 40.0, '2(5)': 10.0, '(2)(3)': 6.0, '(1+2)4': 12.0, '2(3+4)': 14.0, '(1+2)(3+4)': 21.0, '2 (5) * 4': 40.0, '((2))(3)': 6.0, '-2(5)': -10.0, '2(5)^2': 50.0, '10/2(5)': 25.0};
    desktop.forEach((e, v) => expect(evaluate(e, const {}), v, reason: e));
    expect(evaluate('x(2+1)', const {'x': 3}), 9);
    expect(evaluate('2x(4)', const {'x': 3}), 24);
    expect(evaluate('foo(2)', const {}), isNull);
    expect(evaluate('((2))', const {}), isNull);
    expect(evaluate('5', const {}), isNull);
  });
}
