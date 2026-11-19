-- Только одна ветка: configuration / kind / object / module / unconfirmed.
-- Параметры :level, :parent_id, :kind, :module_path задаёт вызывающий тест.
-- Синтетические узлы видов не хранятся в отдельной таблице дерева.
SELECT 'kind' AS node_type, kind AS node_key, kind AS name,
       NULL AS status, NULL AS reason_code
FROM objects WHERE :level = 'configuration' AND parent_id = :parent_id
GROUP BY kind
UNION ALL
SELECT 'object', CAST(object_id AS TEXT), name, NULL, NULL
FROM objects WHERE :level = 'kind' AND parent_id = :parent_id AND kind = :kind
UNION ALL
SELECT 'module', path, path, parse_status, reason_code
FROM modules WHERE :level = 'object' AND owner_id = :parent_id
UNION ALL
SELECT 'module', path, path, parse_status, reason_code
FROM modules WHERE :level = 'unconfirmed' AND owner_id IS NULL
UNION ALL
SELECT 'method', CAST(method_id AS TEXT), name, 'accepted', NULL
FROM methods WHERE :level = 'module' AND module_path = :module_path
ORDER BY node_type, name COLLATE BINARY, node_key;
