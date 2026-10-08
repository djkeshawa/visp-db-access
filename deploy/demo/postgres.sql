CREATE TABLE customers (id bigint PRIMARY KEY, name text NOT NULL, email text NOT NULL, phone text);
CREATE TABLE products (id bigint PRIMARY KEY, name text NOT NULL, price numeric(12,2) NOT NULL);
CREATE TABLE orders (id bigint PRIMARY KEY, customer_id bigint NOT NULL REFERENCES customers(id), ordered_at timestamptz NOT NULL, status text NOT NULL);
CREATE TABLE order_items (id bigint PRIMARY KEY, order_id bigint NOT NULL REFERENCES orders(id), product_id bigint NOT NULL REFERENCES products(id), quantity integer NOT NULL, unit_price numeric(12,2) NOT NULL);
INSERT INTO customers SELECT n,'Customer '||n,'customer'||n||'@example.com','+1555'||lpad(n::text,7,'0') FROM generate_series(1,3000) n;
INSERT INTO products SELECT n,'Product '||n,(n%100+1)*1.99 FROM generate_series(1,300) n;
INSERT INTO orders SELECT n,(n%3000)+1,now()-(n%90)*interval '1 day',CASE WHEN n%3=0 THEN 'pending' ELSE 'shipped' END FROM generate_series(1,9000) n;
INSERT INTO order_items SELECT n,(n%9000)+1,(n%300)+1,(n%5)+1,((n%300)%100+1)*1.99 FROM generate_series(1,27000) n;
CREATE INDEX orders_customer ON orders(customer_id);
CREATE INDEX items_order ON order_items(order_id);
-- The gateway defaults to read-only transactions. For a read-only demo target
-- account, create a separate user and grant SELECT instead of using this owner.
