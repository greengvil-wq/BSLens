"""Только авторская синтетическая фикстура S2; не сборщик проекта/Git."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
ACCEPTED = 'Catalogs/Items/Ext/ObjectModule.bsl'
UNPARSED = 'Documents/Order/Ext/ObjectModule.bsl'
ORPHAN = 'Unconfirmed.bsl'
ROOT_MODULES = ('Ext/ManagedApplicationModule.bsl', 'Ext/SessionModule.bsl')


def xml(kind, name, children=''):
    identity = {'SyntheticV5': 1, 'Items': 2, 'Empty': 3, 'Order': 4}[name]
    return (f'<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.21">\n'
            f'  <{kind} uuid="00000000-0000-0000-0000-{identity:012d}"><Properties><Name>{name}</Name></Properties>'
            f'<ChildObjects>{children}</ChildObjects></{kind}>\n'
            '</MetaDataObject>\n').encode()


# Литералы служат единственным источником байтов: локальные выгрузки не читаются.
SOURCES = {
    'Configuration.xml': xml('Configuration', 'SyntheticV5',
        '<Catalog>Items</Catalog><Catalog>Empty</Catalog><Document>Order</Document>'),
    'Catalogs/Items.xml': xml('Catalog', 'Items'),
    'Catalogs/Empty.xml': xml('Catalog', 'Empty'),
    'Documents/Order.xml': xml('Document', 'Order'),
    ACCEPTED: b'\xef\xbb\xbf' + ('  Процедура ЯПервый() Экспорт\r\n  КонецПроцедуры\r\n'
        '  Процедура АВторой()\r\n  КонецПроцедуры\r\n').encode(),
    ROOT_MODULES[0]: b'// synthetic managed application module\n',
    ROOT_MODULES[1]: b'// synthetic session module\n',
    UNPARSED: b'\xef\xbb\xbf' + '// Синтетика\r\n#Область\r\nПоследняяСтрока'.encode(),
    ORPHAN: b'// synthetic, ownership unconfirmed\n',
}


def physical_lines(content):
    """LF завершает строку; CR и BOM сохраняются; после последнего LF нет фантома."""
    start = 0
    number = 1
    while start < len(content):
        lf = content.find(b'\n', start)
        end = len(content) if lf < 0 else lf + 1
        yield number, start, end
        number += 1
        start = end


def populate(db):
    db.executescript((ROOT / 'schema.sql').read_text())
    manifest = {'selected': sorted(SOURCES), 'excluded': [], 'origin': 'synthetic-literals'}
    db.execute('INSERT INTO metadata VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)', (
        1, 5, 'linux-v5-spike-xml221-bsl-r09', 'bslens-v5-s2-fixture', '2026-11-19T00:00:00Z', None,
        'synthetic:s2', '0' * 40, '0' * 40, 's2-fixture', 'xml221', '2.21', 'r09',
        'synthetic-s2-only', '{}', json.dumps(manifest, sort_keys=True),
        json.dumps({'temporary': True, 'objects': 4, 'modules': 5,
                    'accepted': 4, 'unparsed': 1, 'owner_unconfirmed': 1}),
        '{"temporary":true,"defaults":null}', 'not_analyzed'))
    for path, content in sorted(SOURCES.items()):
        kind = 'bsl' if path.endswith('.bsl') else 'xml'
        blob = b'blob ' + str(len(content)).encode() + b'\0' + content
        db.execute('INSERT INTO sources VALUES (?,?,?,?,?,?)',
                   (path, kind, '100644', hashlib.sha1(blob).hexdigest(),
                    hashlib.sha256(content).hexdigest(), content))
        if kind == 'bsl':
            db.executemany('INSERT INTO source_lines(path,line_number,byte_start,byte_end) VALUES (?,?,?,?)',
                           ((path, *line) for line in physical_lines(content)))
    db.executemany('INSERT INTO objects(object_id,parent_id,kind,name,xml_path) VALUES (?,?,?,?,?)', [
        (1, None, 'Configuration', 'SyntheticV5', 'Configuration.xml'),
        (2, 1, 'Catalog', 'Items', 'Catalogs/Items.xml'),
        (3, 1, 'Document', 'Order', 'Documents/Order.xml'),
        (4, 1, 'Catalog', 'Empty', 'Catalogs/Empty.xml'),
    ])
    db.executemany('INSERT INTO modules(path,owner_id,parse_status,reason_code) VALUES (?,?,?,?)', [
        (ACCEPTED, 2, 'accepted', None),
        (UNPARSED, 3, 'unparsed', 'RegionHeader'),
        (ORPHAN, None, 'accepted', None),
        (ROOT_MODULES[1], 1, 'accepted', None),
        (ROOT_MODULES[0], 1, 'accepted', None),
    ])
    # Координаты вручную заданных фактов: токен Процедура … КонецПроцедуры.
    # Это не парсер; пробелы, BOM и CRLF не входят в лексические границы.
    data = SOURCES[ACCEPTED]
    first_end = data.index('КонецПроцедуры'.encode()) + len('КонецПроцедуры'.encode())
    second_start = data.index('Процедура АВторой'.encode())
    second_end = data.rindex('КонецПроцедуры'.encode()) + len('КонецПроцедуры'.encode())
    db.executemany('INSERT INTO methods(method_id,module_path,kind,name,exported,start_line,end_line,byte_start,byte_end) VALUES (?,?,?,?,?,?,?,?,?)', [
        (1, ACCEPTED, 'procedure', 'ЯПервый', 1, 1, 2, 5, first_end),
        (2, ACCEPTED, 'procedure', 'АВторой', 0, 3, 4, second_start, second_end),
    ])
    db.commit()


def read_range(db, path, start_line, end_line):
    row = db.execute((ROOT / 'range.sql').read_text(),
                     dict(path=path, start_line=start_line, end_line=end_line)).fetchone()
    if row is None:
        raise ValueError('Диапазон отсутствует')
    rowid, start, end = row
    with db.blobopen('sources', 'content', rowid, readonly=True) as blob:
        blob.seek(start)
        return blob.read(end - start)
