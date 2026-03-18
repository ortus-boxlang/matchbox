-- MatchBox datasource integration test schema and seed data

CREATE TABLE IF NOT EXISTS users (
    id         SERIAL PRIMARY KEY,
    name       VARCHAR(100) NOT NULL,
    email      VARCHAR(255) UNIQUE NOT NULL,
    age        INTEGER,
    active     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS products (
    id          SERIAL PRIMARY KEY,
    name        VARCHAR(200) NOT NULL,
    price       NUMERIC(10, 2) NOT NULL,
    in_stock    BOOLEAN NOT NULL DEFAULT TRUE,
    description TEXT
);

CREATE TABLE IF NOT EXISTS orders (
    id         SERIAL PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    product_id INTEGER NOT NULL REFERENCES products(id),
    quantity   INTEGER NOT NULL DEFAULT 1,
    total      NUMERIC(10, 2) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Seed users (inserted in a fixed order so ORDER BY id is predictable)
INSERT INTO users (name, email, age, active) VALUES
    ('Alice Johnson', 'alice@example.com',   30, TRUE),
    ('Bob Smith',     'bob@example.com',     25, TRUE),
    ('Carol White',   'carol@example.com',   35, TRUE),
    ('David Brown',   'david@example.com',   28, FALSE),
    ('Eve Davis',     'eve@example.com',     42, TRUE);

-- Seed products
INSERT INTO products (name, price, in_stock, description) VALUES
    ('BoxLang Pro',       99.99,  TRUE,  'Professional BoxLang license'),
    ('Matchbox Addon',    29.99,  TRUE,  'Matchbox runtime extension'),
    ('Legacy Package',     9.99,  FALSE, 'Discontinued legacy package'),
    ('Enterprise Suite', 499.00,  TRUE,  'Enterprise BoxLang suite'),
    ('Community Edition',  0.00,  TRUE,  'Free community version');

-- Seed orders
INSERT INTO orders (user_id, product_id, quantity, total) VALUES
    (1, 1, 1,  99.99),
    (1, 2, 2,  59.98),
    (2, 5, 1,   0.00),
    (3, 4, 1, 499.00),
    (5, 1, 1,  99.99);
