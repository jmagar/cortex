use std::error::Error;

use anyhow::anyhow;

use super::*;

#[test]
fn service_error_display_uses_user_facing_message() {
    assert_eq!(
        ServiceError::InvalidInput("bad timestamp".into()).to_string(),
        "bad timestamp"
    );
    assert_eq!(
        ServiceError::Busy("database worker limit reached".into()).to_string(),
        "database worker limit reached"
    );
}

#[test]
fn anyhow_errors_convert_to_internal_service_errors() {
    let err: ServiceError = anyhow!("database failed").into();

    assert!(matches!(err, ServiceError::Internal(_)));
    assert_eq!(err.to_string(), "database failed");
    assert!(Error::source(&err).is_none());
}

#[test]
fn typed_variants_display_correctly() {
    assert_eq!(
        ServiceError::DatabaseTimeout.to_string(),
        "database timeout: pool did not yield a connection in time"
    );
    assert_eq!(
        ServiceError::ConstraintViolation {
            message: "UNIQUE constraint failed: logs.id".into()
        }
        .to_string(),
        "constraint violation: UNIQUE constraint failed: logs.id"
    );
    assert_eq!(
        ServiceError::RowNotFound.to_string(),
        "row not found"
    );
    assert_eq!(
        ServiceError::NotFound("no such host".into()).to_string(),
        "no such host"
    );
}

#[test]
fn typed_variants_match_without_downcasting() {
    let err = ServiceError::DatabaseTimeout;
    assert!(matches!(err, ServiceError::DatabaseTimeout));

    let err = ServiceError::ConstraintViolation {
        message: "unique".into(),
    };
    assert!(matches!(err, ServiceError::ConstraintViolation { .. }));

    let err = ServiceError::RowNotFound;
    assert!(matches!(err, ServiceError::RowNotFound));
}
