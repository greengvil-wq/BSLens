-- Только одна ветка; порядок корня соответствует R09: модули, виды, неподтверждённые.
-- Параметры :level, :parent_id, :kind, :module_path задаёт вызывающий тест.
WITH branch(node_type,node_key,name,status,reason_code,section,position) AS (
SELECT 'module', path, path, parse_status, reason_code, 0, 0
FROM modules WHERE :level = 'configuration' AND owner_id = :parent_id
UNION ALL
SELECT 'kind', kind, kind, NULL, NULL, 1, 0
FROM objects WHERE :level = 'configuration' AND parent_id = :parent_id GROUP BY kind
UNION ALL
SELECT 'unconfirmed', 'unconfirmed', 'Владелец не подтверждён', NULL, NULL, 2, 0
WHERE :level = 'configuration' AND EXISTS (SELECT 1 FROM modules WHERE owner_id IS NULL)
UNION ALL
SELECT 'object', CAST(object_id AS TEXT), name, NULL, NULL, 0, 0
FROM objects WHERE :level = 'kind' AND parent_id = :parent_id AND kind = :kind
UNION ALL
SELECT 'module', path, path, parse_status, reason_code, 0, 0
FROM modules WHERE :level = 'object' AND owner_id = :parent_id
UNION ALL
SELECT 'module', path, path, parse_status, reason_code, 0, 0
FROM modules WHERE :level = 'unconfirmed' AND owner_id IS NULL
UNION ALL
SELECT 'method', CAST(method_id AS TEXT), name, 'accepted', NULL, 0, start_line
FROM methods WHERE :level = 'module' AND module_path = :module_path
)
SELECT node_type,node_key,name,status,reason_code FROM branch
ORDER BY section, position, name COLLATE BINARY, node_key;
