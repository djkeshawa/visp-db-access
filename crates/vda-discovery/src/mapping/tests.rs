use super::*;
use aws_sdk_rds::types::{DbClusterMember, DbSubnetGroup, Endpoint};

fn cluster(identifier: &str) -> DbCluster {
    cluster_builder(identifier).build()
}

fn cluster_builder(identifier: &str) -> aws_sdk_rds::types::builders::DbClusterBuilder {
    DbCluster::builder()
        .db_cluster_identifier(identifier)
        .db_cluster_arn(format!(
            "arn:aws:rds:us-east-1:123456789012:cluster:{identifier}"
        ))
        .engine("aurora-postgresql")
        .engine_version("16.4")
        .endpoint("writer.example.com")
        .reader_endpoint("reader.example.com")
        .port(5432)
        .database_name("shop")
        .status("available")
        .storage_encrypted(true)
        .multi_az(true)
        .iam_database_authentication_enabled(true)
        .db_cluster_members(
            DbClusterMember::builder()
                .db_instance_identifier("writer")
                .is_cluster_writer(true)
                .build(),
        )
}

fn instance(identifier: &str) -> DbInstance {
    instance_builder(identifier).build()
}

fn instance_builder(identifier: &str) -> aws_sdk_rds::types::builders::DbInstanceBuilder {
    DbInstance::builder()
        .db_instance_identifier(identifier)
        .db_instance_arn(format!(
            "arn:aws:rds:us-east-1:123456789012:db:{identifier}"
        ))
        .engine("mysql")
        .engine_version("8.0.40")
        .db_instance_status("available")
        .db_name("shop")
        .publicly_accessible(true)
        .storage_encrypted(true)
        .multi_az(true)
        .iam_database_authentication_enabled(true)
        .db_subnet_group(DbSubnetGroup::builder().vpc_id("vpc-123").build())
        .endpoint(
            Endpoint::builder()
                .address(format!("{identifier}.example.com"))
                .port(3306)
                .build(),
        )
}

#[test]
fn supported_engine_allowlist_is_exhaustive() {
    for raw in ["postgres", "aurora-postgresql"] {
        assert_eq!(supported_engine(raw), Some("postgres"));
    }
    for raw in ["mysql", "aurora-mysql", "aurora", "mariadb"] {
        assert_eq!(supported_engine(raw), Some("mysql"));
    }
    for raw in [
        "oracle-ee",
        "sqlserver-ex",
        "db2-ae",
        "docdb",
        "neptune",
        "unknown",
    ] {
        assert_eq!(supported_engine(raw), None);
    }
}

#[test]
fn aurora_reader_requires_actual_reader_member() {
    let db = cluster("shop-prod");
    let mapped = map_cluster(&db, "us-east-1").unwrap();
    assert_eq!(mapped.replica_host, None);
    assert_eq!(mapped.database.as_deref(), Some("shop"));
    assert_eq!(mapped.account_id, "123456789012");
    assert_eq!(mapped.engine_version, "16.4");
    assert!(mapped.encrypted && mapped.multi_az && mapped.iam_auth_enabled);
    let db = cluster_builder("shop-prod")
        .db_cluster_members(DbClusterMember::builder().is_cluster_writer(false).build())
        .build();
    let mapped = map_cluster(&db, "us-east-1").unwrap();
    assert_eq!(mapped.replica_host.as_deref(), Some("reader.example.com"));
    assert_eq!(mapped.replica_port, Some(5432));
}

#[test]
fn instances_include_metadata_and_nonavailable_endpoints() {
    let db = instance_builder("shop-stg")
        .db_instance_status("modifying")
        .tag_list(Tag::builder().key("Owner").value("Shop").build())
        .build();
    let mapped = map_instance(&db, "eu-west-1").unwrap();
    assert_eq!(mapped.status_detail, "modifying");
    assert_eq!(mapped.region, "eu-west-1");
    assert_eq!(mapped.vpc_id.as_deref(), Some("vpc-123"));
    assert_eq!(mapped.tags.get("Owner").map(String::as_str), Some("Shop"));
    assert!(
        mapped.publicly_accessible
            && mapped.encrypted
            && mapped.multi_az
            && mapped.iam_auth_enabled
    );
    assert_eq!(mapped.suggested_environment, "staging");
    assert!(map_instance(
        &instance_builder("shop-stg").set_endpoint(None).build(),
        "eu-west-1"
    )
    .is_none());
}

#[test]
fn malformed_ports_and_unsupported_engines_are_counted() {
    let records = vec![
        instance_builder("shop").engine("docdb").build(),
        instance_builder("shop").set_endpoint(None).build(),
        instance_builder("shop")
            .endpoint(
                Endpoint::builder()
                    .address("shop.example.com")
                    .port(0)
                    .build(),
            )
            .build(),
        instance_builder("shop")
            .endpoint(
                Endpoint::builder()
                    .address("shop.example.com")
                    .port(70000)
                    .build(),
            )
            .build(),
    ];
    let result = map_region(&[], &records, "us-east-1");
    assert!(result.resources.is_empty());
    assert_eq!(result.skipped, 4);
}

#[test]
fn members_and_replicas_are_deduplicated() {
    let cluster = cluster("shop-prod");
    let member = instance_builder("member")
        .db_cluster_identifier("shop-prod")
        .build();
    let primary = instance("orders");
    let replicas = ["z-replica", "a-replica"].map(|name| {
        instance_builder(name)
            .read_replica_source_db_instance_identifier("orders")
            .build()
    });
    let pending = instance_builder("0-pending")
        .read_replica_source_db_instance_identifier("orders")
        .db_instance_status("creating")
        .build();
    let mut records = vec![member, primary, pending];
    records.extend(replicas);
    let outcome = map_region(&[cluster], &records, "us-east-1");
    assert_eq!(outcome.resources.len(), 2);
    let primary = outcome
        .resources
        .iter()
        .find(|db| db.identifier == "orders")
        .unwrap();
    assert_eq!(
        primary.replica_host.as_deref(),
        Some("a-replica.example.com")
    );
    let aurora = outcome
        .resources
        .iter()
        .find(|db| db.kind == "aurora_cluster")
        .unwrap();
    assert_eq!(aurora.vpc_id.as_deref(), Some("vpc-123"));
    assert!(aurora.publicly_accessible);
}

#[test]
fn cross_region_source_arn_is_used_for_replica_lookup() {
    let primary = instance("orders");
    let replica = instance_builder("replica")
        .read_replica_source_db_instance_identifier(primary.db_instance_arn().unwrap())
        .build();
    let outcome = map_region(&[], &[primary, replica], "us-east-1");
    assert_eq!(outcome.resources.len(), 1);
    assert_eq!(
        outcome.resources.first().unwrap().replica_host.as_deref(),
        Some("replica.example.com")
    );
}

#[test]
fn relative_replica_identifiers_do_not_collide_across_accounts_or_regions() {
    let primary = instance("orders");
    let other_account = instance_builder("replica")
        .db_instance_arn("arn:aws:rds:us-east-1:999999999999:db:replica")
        .read_replica_source_db_instance_identifier("orders")
        .build();
    let other_region = instance_builder("replica")
        .db_instance_arn("arn:aws:rds:eu-west-1:123456789012:db:replica")
        .read_replica_source_db_instance_identifier("orders")
        .build();
    let outcome = map_region(&[], &[primary, other_account, other_region], "us-east-1");
    assert_eq!(outcome.resources.len(), 1);
    assert_eq!(outcome.resources.first().unwrap().replica_host, None);
}

#[test]
fn environment_key_order_case_normalization_and_name_boundaries() {
    let keys = ["environment", "env", "stage"].map(String::from);
    for (value, expected) in [
        (" PRODUCTION ", "production"),
        ("prd", "production"),
        ("live", "production"),
        ("stg", "staging"),
        ("UAT", "staging"),
        ("qa", "staging"),
        ("preprod", "staging"),
        ("dev", "development"),
        ("development", "development"),
        ("test", "development"),
        ("sandbox", "development"),
    ] {
        let tags = BTreeMap::from([
            ("ENVIRONMENT".into(), value.into()),
            ("env".into(), "test".into()),
        ]);
        assert_eq!(suggested_environment(&tags, "shop-prod", &keys), expected);
    }
    for (name, expected) in [
        ("prod-shop", "production"),
        ("shop-stg", "staging"),
        ("shop-dev", "development"),
        ("dev-shop", "development"),
        ("product", "production"),
        ("device", "production"),
        ("unknown", "production"),
    ] {
        assert_eq!(
            suggested_environment(&BTreeMap::new(), name, &keys),
            expected
        );
    }
    let tags = BTreeMap::from([("TeamStage".into(), "staging".into())]);
    assert_eq!(
        suggested_environment(&tags, "shop-prod", &["teamstage".into()]),
        "staging"
    );
}
