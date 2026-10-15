-- 自定义排序：为 games 增加 sort_order 列。
-- 存量数据按插入顺序（rowid）回填，保证升级后显示顺序不变。
ALTER TABLE games ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
UPDATE games SET sort_order = rowid;
