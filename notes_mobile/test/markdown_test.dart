// Import to Notes: Markdown as a note's body (share.dart markdownBody).
import 'package:flutter_test/flutter_test.dart';
import 'package:notes/share.dart';

void main() {
  test('headings, lists, checklists, quotes and emphasis', () {
    final b = markdownBody('# Trip\n## Packing\n- [x] Passport\n- [ ] **Charger**\n* socks\n1. book\n> quote\nplain *word*\n');
    expect([for (final p in b) p['st']], ['title', 'heading', 'check', 'check', 'bullet', 'number', 'body', 'body']);
    expect(b[2]['ck'], true);
    expect(b[3]['sp'], [{'t': 'Charger', 'b': true}]);
    expect(b[6]['q'], true);
    expect(b[7]['sp'], [{'t': 'plain '}, {'t': 'word', 'i': true}]);
  });
}
