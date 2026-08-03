import pytest
from fastapi.testclient import TestClient

from app.main import app

client = TestClient(app)
AUTH = {"Authorization": "Bearer dev-token"}


def test_health_ok() -> None:
    response = client.get("/health")
    assert response.status_code == 200
    assert response.json()["status"] == "ok"


def test_create_job_requires_auth() -> None:
    response = client.post(
        "/v1/jobs",
        json={
            "meeting_id": "m1",
            "kind": "brief",
            "primary_language": "ru",
            "allowed_languages": ["ru", "en"],
        },
    )
    assert response.status_code == 401


def test_job_roundtrip(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("LLM_BASE_URL", raising=False)
    created = client.post(
        "/v1/jobs",
        headers=AUTH,
        json={
            "meeting_id": "m1",
            "kind": "brief",
            "primary_language": "ru",
            "allowed_languages": ["ru", "en", "es"],
        },
    )
    assert created.status_code == 201
    job = created.json()
    assert job["status"] == "succeeded"
    assert len(job["artifact_ids"]) == 1

    fetched = client.get(f"/v1/jobs/{job['id']}", headers=AUTH)
    assert fetched.status_code == 200
    assert fetched.json()["id"] == job["id"]

    artifact = client.get(f"/v1/artifacts/{job['artifact_ids'][0]}", headers=AUTH)
    assert artifact.status_code == 200
    assert "Stub brief" in artifact.json()["body_markdown"]
