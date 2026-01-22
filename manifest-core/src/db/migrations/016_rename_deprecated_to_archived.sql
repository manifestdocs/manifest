-- Rename 'deprecated' state to 'archived' for soft delete semantics
UPDATE features SET state = 'archived' WHERE state = 'deprecated';
