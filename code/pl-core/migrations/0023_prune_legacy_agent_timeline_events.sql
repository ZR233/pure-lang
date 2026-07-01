DELETE FROM agent_events
WHERE kind NOT IN (
    'spawned',
    'messageQueued',
    'followupStarted',
    'waitCompleted',
    'closed'
);
