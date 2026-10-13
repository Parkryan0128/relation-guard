#!/usr/bin/env python3
"""HTTP client and grant/revoke workflow used by the integration tests."""

import json
import os
import pathlib
import time
import urllib.error
import urllib.request
import uuid


class AuthorizationError(Exception):
    pass


class RevisionConflict(AuthorizationError):
    pass


class Client:
    def __init__(self, base, tenant, token, actor):
        self.base = f"{base}/v1/tenants/{tenant}"
        self.token = token
        self.actor = actor

    def request(self, path, body=None, key=None):
        headers = {
            "X-Actor-Id": self.actor,
            "Authorization": "Bearer " + self.token,
            "Content-Type": "application/json",
        }
        if key:
            headers["Idempotency-Key"] = key
        encoded = None if body is None else json.dumps(body).encode()
        # A write retry keeps exactly the same body and key. A conflict needs a new decision.
        for attempt in range(3):
            try:
                with urllib.request.urlopen(
                    urllib.request.Request(
                        self.base + "/" + path, data=encoded, headers=headers
                    ),
                    timeout=3,
                ) as response:
                    return json.load(response)
            except urllib.error.HTTPError as error:
                try:
                    with error:
                        result = json.load(error)
                except (ValueError, TypeError) as invalid:
                    raise AuthorizationError("Invalid error response") from invalid
                if (
                    not isinstance(result, dict)
                    or not isinstance(result.get("error"), dict)
                    or not isinstance(result["error"].get("code"), str)
                ):
                    raise AuthorizationError("Invalid error response")
                code = result["error"]["code"]
                if code == "REVISION_CONFLICT":
                    raise RevisionConflict(result)
                if (
                    not key
                    or code
                    not in (
                        "WRITE_OUTCOME_UNKNOWN",
                        "DATABASE_UNAVAILABLE",
                        "OVERLOADED",
                    )
                    or attempt == 2
                ):
                    raise AuthorizationError(result)
            except (OSError, ValueError) as error:
                if not key or attempt == 2:
                    raise AuthorizationError("No verified result") from error
            time.sleep(0.1 * (attempt + 1))
        raise AuthorizationError("Retry budget exhausted")

    def can_view(self, user, document):
        result = self.request(
            "check", {"subject": user, "resource": document, "action": "view"}
        )
        if not isinstance(result, dict):
            raise AuthorizationError("Invalid decision response")
        revision = result.get("evaluated_revision")
        if (
            result.get("decision") not in ("allow", "deny")
            or result.get("model_version") != "org-doc-v2"
            or not isinstance(revision, str)
            or not revision.isascii()
            or not revision.isdecimal()
            or len(revision) > 19
            or (len(revision) > 1 and revision[0] == "0")
            or int(revision) > 9223372036854775807
        ):
            raise AuthorizationError("Invalid decision response")
        try:
            uuid.UUID(result["request_id"])
        except (KeyError, ValueError, TypeError, AttributeError) as error:
            raise AuthorizationError("Invalid request ID") from error
        if result["decision"] == "allow":
            if result.get("reason") not in ("MATCHING_RELATION", "ORG_ADMIN"):
                raise AuthorizationError("Invalid allow reason")
            return True
        if result.get("reason") not in ("NOT_FOUND", "NO_MATCHING_RELATION"):
            raise AuthorizationError("Invalid deny reason")
        return False


def main():
    credentials = json.loads(
        pathlib.Path(
            os.environ.get("AUTHZ_CLIENT_FILE", "secrets/client.json")
        ).read_text()
    )
    client = Client(
        os.environ.get("AUTHZ_URL", "http://127.0.0.1:8080"),
        credentials["tenant_id"],
        credentials["token"],
        credentials["actor_id"],
    )
    suffix = uuid.uuid4().hex
    manager = {"kind": "user", "id": "manager-" + suffix}
    viewer = {"kind": "user", "id": "editor-" + suffix}
    doc = {"kind": "document", "id": "demo-" + suffix}
    management = {
        "resource": doc,
        "relation": "manager",
        "subject": manager,
        "subject_relation": "",
    }
    revision = client.request("revision")["revision"]
    created = client.request(
        "write",
        {
            "expected_revision": revision,
            "operations": [
                {"op": "put_entity", "entity": manager},
                {"op": "put_entity", "entity": viewer},
                {"op": "put_entity", "entity": doc},
                {"op": "put_relation", "relationship": management},
            ],
        },
        str(uuid.uuid4()),
    )
    # The test simulates a trusted backend with already-authenticated users.
    # Real applications derive these actor IDs from login sessions, never user input.
    managing = Client(
        os.environ.get("AUTHZ_URL", "http://127.0.0.1:8080"),
        credentials["tenant_id"],
        credentials["token"],
        manager["id"],
    )
    editing = Client(
        os.environ.get("AUTHZ_URL", "http://127.0.0.1:8080"),
        credentials["tenant_id"],
        credentials["token"],
        viewer["id"],
    )
    edge = {
        "resource": doc,
        "relation": "editor",
        "subject": viewer,
        "subject_relation": "",
    }
    changed = managing.request(
        "write",
        {
            "expected_revision": created["revision"],
            "operations": [{"op": "put_relation", "relationship": edge}],
        },
        str(uuid.uuid4()),
    )
    assert editing.can_view(viewer, doc)
    print("Granted: allow; the document manager shared access.")
    try:
        editing.request(
            "write",
            {
                "expected_revision": changed["revision"],
                "operations": [
                    {
                        "op": "put_relation",
                        "relationship": {**edge, "relation": "manager"},
                    }
                ],
            },
            str(uuid.uuid4()),
        )
    except AuthorizationError as error:
        if (
            not error.args
            or not isinstance(error.args[0], dict)
            or error.args[0].get("error", {}).get("code") != "FORBIDDEN"
        ):
            raise
        print("Editor cannot share: forbidden; no self-promotion.")
    else:
        raise AssertionError("Editor unexpectedly changed permissions")
    managing.request(
        "write",
        {
            "expected_revision": changed["revision"],
            "operations": [{"op": "delete_relation", "relationship": edge}],
        },
        str(uuid.uuid4()),
    )
    assert not editing.can_view(viewer, doc)
    print("Revoked: deny; protected operation blocked.")


if __name__ == "__main__":
    main()
