// The library as the folders and notes lists show it (nt-ffi `library`): accounts, folders,
// Smart Folders' notes, tags and each note's list facts; the places a list can show and how
// it sorts and groups them (Pinned, Today, Yesterday, Previous 7 Days, Previous 30 Days,
// then by month and year), and the list's dates.
class Folder {
  Folder(Map<String, dynamic> j)
    : id = j['id'] as String,
      account = j['account'] as String? ?? '',
      name = j['name'] as String? ?? '',
      parent = j['parent'] as String?,
      kind = j['kind'] as String? ?? 'normal',
      isDefault = j['default'] == true,
      order = (j['order'] as num?)?.toInt() ?? 0,
      query = j['query'] as String?;
  final String id, account, name, kind;
  final String? parent;

  /// A Smart Folder's rules (Notes' query JSON).
  final String? query;
  final bool isDefault;
  final int order;
  bool get smart => kind == 'smart';
  bool get trash => kind == 'trash';
}

/// An account: this library's own, or one a Mac's library brought (its iCloud, On My Mac).
class Account {
  Account(Map<String, dynamic> j) : id = j['id'] as String? ?? '', name = j['name'] as String? ?? '';
  final String id, name;

  /// Ours (nt-core's local account) or a Mac's On My Mac: shown as this device's.
  bool get local => id == 'LocalAccount' || name.trim().isEmpty || name.startsWith('On ');
}

class NoteMeta {
  NoteMeta(Map<String, dynamic> j)
    : id = j['id'] as String,
      folder = j['folder'] as String? ?? '',
      title = j['title'] as String? ?? '',
      snippet = j['snippet'] as String? ?? '',
      created = (j['created'] as num?)?.toDouble() ?? 0,
      modified = (j['modified'] as num?)?.toDouble() ?? 0,
      pinned = j['pinned'] == true,
      locked = j['locked'] == true,
      deleted = j['deleted'] == true,
      notDownloaded = j['nd'] == true,
      thumb = j['thumb'] as String?,
      tags = ((j['tags'] as List?) ?? const []).cast<String>();
  final String id, folder, title, snippet;
  final double created, modified;
  final bool pinned, locked, deleted, notDownloaded;
  final String? thumb;
  final List<String> tags;
}

/// What a notes list shows.
sealed class Place {
  const Place();
}

class AllNotes extends Place {
  /// Every note, or (`account`) an account's: "All iCloud".
  const AllNotes([this.account]);
  final String? account;
  @override
  bool operator ==(Object o) => o is AllNotes && o.account == account;
  @override
  int get hashCode => 1 ^ account.hashCode;
}

class InFolder extends Place {
  const InFolder(this.id);
  final String id;
  @override
  bool operator ==(Object o) => o is InFolder && o.id == id;
  @override
  int get hashCode => id.hashCode;
}

class WithTag extends Place {
  /// Notes' tag browser: the tags chosen (`include`, lower case; `tag` alone for one), the ones
  /// struck out (`exclude`), and whether a note needs all the chosen ones or any.
  /// No tags chosen: every tagged note.
  const WithTag(this.tag, {this.include = const {}, this.exclude = const {}, this.all = false});
  final String tag;
  final Set<String> include, exclude;
  final bool all;

  Set<String> get chosen => include.isEmpty && tag.isNotEmpty ? {tag.toLowerCase()} : include;

  bool matches(List<String> tags) {
    final t = {for (final x in tags) x.toLowerCase()};
    if (t.isEmpty || t.any(exclude.contains)) return false;
    final c = chosen;
    if (c.isEmpty) return true;
    return all ? c.every(t.contains) : c.any(t.contains);
  }

  String get _key => '${(chosen.toList()..sort()).join(',')}|${(exclude.toList()..sort()).join(',')}|$all';
  @override
  bool operator ==(Object o) => o is WithTag && o._key == _key;
  @override
  int get hashCode => _key.hashCode ^ 7;
}

class Library {
  Library(Map<String, dynamic> j)
    : folders = [for (final f in (j['folders'] as List? ?? const [])) Folder((f as Map).cast())],
      notes = [for (final n in (j['notes'] as List? ?? const [])) NoteMeta((n as Map).cast())],
      smart = {for (final e in ((j['smart'] as Map?) ?? const {}).entries) e.key as String: ((e.value as List).cast<String>()).toSet()},
      accounts = [for (final a in (j['accounts'] as List? ?? const [])) Account((a as Map).cast())],
      accountName = (((j['accounts'] as List?)?.firstOrNull as Map?)?['name'] as String?) ?? 'On My Phone';
  Library.empty() : folders = const [], notes = const [], smart = const {}, accounts = const [], accountName = '';

  final List<Folder> folders;
  final List<NoteMeta> notes;
  final Map<String, Set<String>> smart;
  final List<Account> accounts;
  final String accountName;

  /// The accounts the sidebar shows a section for: this device's first, then the others (a
  /// Mac's iCloud) in the library's order; one with nothing in it is left out unless it's ours.
  List<Account> get shownAccounts {
    final l = [for (final a in accounts) if (a.id == 'LocalAccount' || folders.any((f) => f.account == a.id)) a];
    l.sort((a, b) => (a.local ? 0 : 1).compareTo(b.local ? 0 : 1));
    return l;
  }

  /// Whether there's more than one account section (otherwise the sidebar is as before).
  bool get manyAccounts => shownAccounts.length > 1;

  /// The account a place is in (its folder's, or an account's All), if any.
  String? accountOf(Place p) => switch (p) {
    AllNotes(:final account) => account,
    InFolder(:final id) => folder(id)?.account,
    WithTag() => null,
  };

  Folder? folder(String id) => folders.where((f) => f.id == id).firstOrNull;
  NoteMeta? note(String id) => notes.where((n) => n.id == id).firstOrNull;

  Folder? get defaultFolder => folders.where((f) => f.isDefault).firstOrNull;
  Folder? get trash => folders.where((f) => f.trash).firstOrNull;

  /// An account's own Notes folder and Recently Deleted (each account has its own).
  Folder? defaultIn(String account) => folders.where((f) => f.isDefault && f.account == account).firstOrNull;
  Folder? trashIn(String account) => folders.where((f) => f.trash && f.account == account).firstOrNull;

  /// Folders under `parent` (null: the top), the Notes folder first, then by name (Notes' order).
  /// (`account`: only that account's)
  List<Folder> children(String? parent, {String? account}) {
    final l = folders.where((f) => f.parent == parent && !f.trash && (account == null || f.account == account)).toList();
    l.sort((a, b) {
      if (a.isDefault != b.isDefault) return a.isDefault ? -1 : 1;
      final qa = a.name == 'Quick Notes' && a.parent == null, qb = b.name == 'Quick Notes' && b.parent == null;
      if (qa != qb) return qa ? -1 : 1;
      return a.name.toLowerCase().compareTo(b.name.toLowerCase());
    });
    return l;
  }

  bool hasChildren(String id) => folders.any((f) => f.parent == id);

  /// Every tag in use, as written first, in name order.
  List<String> get allTags {
    final seen = <String, String>{};
    for (final n in notes) {
      if (n.deleted) continue;
      for (final t in n.tags) {
        seen.putIfAbsent(t.toLowerCase(), () => t);
      }
    }
    final l = seen.values.toList()..sort((a, b) => a.toLowerCase().compareTo(b.toLowerCase()));
    return l;
  }

  List<NoteMeta> notesIn(Place p) => notes.where((n) {
    switch (p) {
      case AllNotes(:final account):
        return !n.deleted && (account == null || folder(n.folder)?.account == account);
      case InFolder(:final id):
        final f = folder(id);
        if (f == null) return false;
        if (f.trash) return n.deleted && (accounts.length < 2 || folder(n.folder)?.account == f.account);
        if (n.deleted) return false;
        if (f.smart) return smart[id]?.contains(n.id) ?? false;
        return n.folder == id;
      case WithTag():
        if (n.deleted) return false;
        return p.matches(n.tags);
    }
  }).toList();

  int count(Place p) => notesIn(p).length;

  /// The account as Notes names it here ("On My Phone", "On My Tablet").
  String account({bool tablet = false}) => accountName.isEmpty || accountName == 'On My Mac' ? (tablet ? 'On My Tablet' : 'On My Phone') : accountName;

  /// An account's section name: this device's ("On My Phone"), or its own ("iCloud").
  String accountTitle(Account a, {bool tablet = false}) => a.local ? (tablet ? 'On My Tablet' : 'On My Phone') : a.name;

  String title(Place p, {bool tablet = false}) => switch (p) {
    AllNotes(:final account) when account != null => 'All ${accountTitle(accounts.firstWhere((a) => a.id == account, orElse: () => Account({'id': account})), tablet: tablet)}',
    AllNotes() => 'All ${account(tablet: tablet)}',
    InFolder(:final id) => folder(id)?.name ?? 'Notes',
    WithTag(:final chosen) =>
      chosen.isEmpty ? 'All Tags' : [for (final c in chosen) '#${allTags.firstWhere((t) => t.toLowerCase() == c, orElse: () => c)}'].join(' '),
  };
}

/// Settings > Sort Notes By.
enum SortBy { edited, created, title }

/// Sorts notes as the list shows them (pinned ones are shown apart, first).
/// `reverse`: Oldest First (dates) or descending (titles).
List<NoteMeta> sortNotes(List<NoteMeta> l, SortBy by, {bool reverse = false}) {
  final out = [...l];
  switch (by) {
    case SortBy.edited:
      out.sort((a, b) => b.modified.compareTo(a.modified));
    case SortBy.created:
      out.sort((a, b) => b.created.compareTo(a.created));
    case SortBy.title:
      out.sort((a, b) => a.title.toLowerCase().compareTo(b.title.toLowerCase()));
  }
  return reverse ? out.reversed.toList() : out;
}

/// The list's sections: Pinned, then the date groups (or one unnamed section without grouping).
List<(String, List<NoteMeta>)> groupNotes(List<NoteMeta> l, SortBy by, {required bool byDate, bool reverse = false, DateTime? now}) {
  final sorted = sortNotes(l, by, reverse: reverse);
  final pinned = sorted.where((n) => n.pinned).toList();
  final rest = sorted.where((n) => !n.pinned).toList();
  final out = <(String, List<NoteMeta>)>[];
  if (pinned.isNotEmpty) out.add(('Pinned', pinned));
  if (!byDate || by == SortBy.title) {
    if (rest.isNotEmpty) out.add((pinned.isEmpty ? '' : 'Notes', rest));
    return out;
  }
  final t = now ?? DateTime.now();
  final today = DateTime(t.year, t.month, t.day);
  String group(NoteMeta n) {
    final d = DateTime.fromMillisecondsSinceEpoch(((by == SortBy.created ? n.created : n.modified) * 1000).round());
    final day = DateTime(d.year, d.month, d.day);
    final ago = today.difference(day).inDays;
    if (ago <= 0) return 'Today';
    if (ago == 1) return 'Yesterday';
    if (ago < 7) return 'Previous 7 Days';
    if (ago < 30) return 'Previous 30 Days';
    if (d.year == t.year) return _months[d.month - 1];
    return '${d.year}';
  }

  for (final n in rest) {
    final g = group(n);
    if (out.isNotEmpty && out.last.$1 == g) {
      out.last.$2.add(n);
    } else {
      out.add((g, [n]));
    }
  }
  return out;
}

const _months = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const _days = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];

/// The list's date: the time today, "Yesterday", the weekday this week, else the date.
String listDate(double seconds, {DateTime? now}) {
  final d = DateTime.fromMillisecondsSinceEpoch((seconds * 1000).round());
  final t = now ?? DateTime.now();
  final ago = DateTime(t.year, t.month, t.day).difference(DateTime(d.year, d.month, d.day)).inDays;
  if (ago <= 0) return clock(d);
  if (ago == 1) return 'Yesterday';
  if (ago < 7) return _days[d.weekday - 1];
  return '${d.month}/${d.day}/${d.year % 100 < 10 ? '0' : ''}${d.year % 100}';
}

String clock(DateTime d) {
  final h = d.hour % 12 == 0 ? 12 : d.hour % 12;
  return '$h:${d.minute.toString().padLeft(2, '0')} ${d.hour < 12 ? 'AM' : 'PM'}';
}

/// The editor's head: "November 4, 2026 at 9:41 AM".
String longDate(double seconds) {
  final d = DateTime.fromMillisecondsSinceEpoch((seconds * 1000).round());
  return '${_months[d.month - 1]} ${d.day}, ${d.year} at ${clock(d)}';
}
