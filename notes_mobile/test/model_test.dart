// The tag browser's filter (model.dart WithTag).
import 'package:flutter_test/flutter_test.dart';
import 'package:notes/model.dart';

void main() {
  test('chosen tags (any or all) and struck-out ones', () {
    expect(const WithTag('').matches(['Work']), true);
    expect(const WithTag('').matches([]), false);
    expect(const WithTag('work').matches(['Work', 'Home']), true);
    expect(const WithTag('', include: {'work', 'home'}).matches(['Work']), true);
    expect(const WithTag('', include: {'work', 'home'}, all: true).matches(['Work']), false);
    expect(const WithTag('', include: {'work', 'home'}, all: true).matches(['home', 'work']), true);
    expect(const WithTag('', include: {'work'}, exclude: {'trip'}).matches(['work', 'Trip']), false);
    expect(const WithTag('', exclude: {'trip'}).matches(['home']), true);
    expect(const WithTag('work') == const WithTag('', include: {'work'}), true);
  });

  test('a Mac\'s accounts each have their section, their All and their Recently Deleted', () {
    final lib = Library({
      'accounts': [
        {'id': 'ICLOUD-1', 'name': 'iCloud', 'kind': 1},
        {'id': 'LocalAccount', 'name': 'On My Mac', 'kind': 3},
      ],
      'folders': [
        {'id': 'n1', 'account': 'LocalAccount', 'name': 'Notes', 'default': true},
        {'id': 't1', 'account': 'LocalAccount', 'name': 'Recently Deleted', 'kind': 'trash'},
        {'id': 'n2', 'account': 'ICLOUD-1', 'name': 'Notes', 'default': true},
        {'id': 'w2', 'account': 'ICLOUD-1', 'name': 'Work'},
        {'id': 't2', 'account': 'ICLOUD-1', 'name': 'Recently Deleted', 'kind': 'trash'},
      ],
      'notes': [
        {'id': 'a', 'folder': 'n1'},
        {'id': 'b', 'folder': 'w2'},
        {'id': 'c', 'folder': 'n2'},
        {'id': 'd', 'folder': 'w2', 'deleted': true},
      ],
    });
    expect(lib.manyAccounts, isTrue);
    expect([for (final a in lib.shownAccounts) lib.accountTitle(a)], ['On My Phone', 'iCloud']);
    expect(lib.count(const AllNotes()), 3);
    expect(lib.count(const AllNotes('ICLOUD-1')), 2);
    expect(lib.title(const AllNotes('ICLOUD-1')), 'All iCloud');
    expect([for (final f in lib.children(null, account: 'ICLOUD-1')) f.name], ['Notes', 'Work']);
    expect(lib.count(InFolder(lib.trashIn('ICLOUD-1')!.id)), 1);
    expect(lib.count(InFolder(lib.trashIn('LocalAccount')!.id)), 0);
    expect(lib.accountOf(const InFolder('w2')), 'ICLOUD-1');
    expect(lib.defaultIn('ICLOUD-1')!.id, 'n2');
  });
}

