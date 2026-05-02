-- VM Instances
CREATE TABLE IF NOT EXISTS vm_instances (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    arch TEXT DEFAULT 'x86_64',
    cpu_cores INTEGER DEFAULT 2,
    ram_mb INTEGER DEFAULT 4096,
    disk_path TEXT NOT NULL,
    disk_size TEXT DEFAULT '40G',
    iso_path TEXT,
    status TEXT DEFAULT 'stopped',
    vnc_port INTEGER,
    qmp_socket TEXT,
    serial_socket TEXT,
    network_mode TEXT DEFAULT 'user',
    audio_enabled INTEGER DEFAULT 0,
    pid INTEGER,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

-- VM Shared Folders
CREATE TABLE IF NOT EXISTS vm_shared_folders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    vm_id TEXT NOT NULL,
    host_path TEXT NOT NULL,
    mount_tag TEXT NOT NULL,
    mount_point TEXT DEFAULT '/mnt/shared',
    readonly INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (vm_id) REFERENCES vm_instances(id) ON DELETE CASCADE
);

-- VM Snapshots
CREATE TABLE IF NOT EXISTS vm_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    vm_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (vm_id) REFERENCES vm_instances(id) ON DELETE CASCADE
);

-- VM Activity Log (what the LLM does in the VM)
CREATE TABLE IF NOT EXISTS vm_activity_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    vm_id TEXT NOT NULL,
    action TEXT NOT NULL,
    input TEXT,
    output TEXT,
    duration_ms INTEGER,
    created_at TEXT DEFAULT (datetime('now')),
    FOREIGN KEY (vm_id) REFERENCES vm_instances(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_vm_activity_vm ON vm_activity_log(vm_id);
CREATE INDEX IF NOT EXISTS idx_vm_activity_time ON vm_activity_log(created_at);
CREATE INDEX IF NOT EXISTS idx_vm_shared_vm ON vm_shared_folders(vm_id);
CREATE INDEX IF NOT EXISTS idx_vm_snapshots_vm ON vm_snapshots(vm_id);
