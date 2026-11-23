-- :end_line включительно; результат — границы, а не загрузка всего BLOB.
-- rowid нужен только локальной тестовой утилите sqlite3.blobopen.
SELECT s.rowid, first.byte_start, last.byte_end
FROM sources AS s
JOIN source_lines AS first ON first.path = s.path AND first.line_number = :start_line
JOIN source_lines AS last ON last.path = s.path AND last.line_number = :end_line
WHERE s.path = :path AND :start_line <= :end_line;
