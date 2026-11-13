-- Align CHECK constraints and defaults with the values the code writes.

-- Project visibility: the API uses 'workspace' and 'private'.
ALTER TABLE projects DROP CONSTRAINT IF EXISTS projects_visibility_check;
UPDATE projects SET visibility = 'private' WHERE visibility = 'restricted';
ALTER TABLE projects ADD CONSTRAINT projects_visibility_check
    CHECK (visibility IN ('workspace','private'));

-- Test plan item categories: the planners also emit smoke and accessibility scenarios.
ALTER TABLE test_plan_items DROP CONSTRAINT IF EXISTS test_plan_items_category_check;
ALTER TABLE test_plan_items ADD CONSTRAINT test_plan_items_category_check
    CHECK (category IN ('functional','negative','boundary','edge_case','regression','integration','security','smoke','accessibility'));

-- Integrations: the control plane emits execution.finished and execution.failed.
ALTER TABLE integrations ALTER COLUMN events SET DEFAULT '{execution.finished}';
UPDATE integrations SET events = array_replace(events, 'execution.completed', 'execution.finished');
