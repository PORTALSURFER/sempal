#!/usr/bin/env python3
import argparse
import datetime as dt
import json
import os
import sqlite3
import sys
from pathlib import Path


EMBEDDING_MODEL_ID = (
    "clap_htsat_fused__sr48k__nfft1024__hop480__mel64__chunk10__repeatpad_v1"
)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Train a softmax logistic regression head from SQLite embeddings."
    )
    parser.add_argument("--db", type=Path, help="Path to library.db")
    parser.add_argument(
        "--model-id",
        default=EMBEDDING_MODEL_ID,
        help="Embedding model_id to train on",
    )
    parser.add_argument(
        "--classes-json",
        type=Path,
        default=Path("assets/ml/classes_v1.json"),
        help="Classes JSON file (default: assets/ml/classes_v1.json)",
    )
    parser.add_argument("--min-confidence", type=float, default=0.85)
    parser.add_argument("--ruleset-version", type=int, default=1)
    parser.add_argument("--use-user-labels", action="store_true")
    parser.add_argument("--seed", type=int, default=123)
    parser.add_argument("--test-fraction", type=float, default=0.1)
    parser.add_argument("--val-fraction", type=float, default=0.1)
    parser.add_argument("--l2", type=float, default=1.0, help="L2 regularization strength")
    parser.add_argument(
        "--class-weight",
        choices=["none", "balanced"],
        default="none",
        help="Class weighting strategy",
    )
    parser.add_argument("--max-iter", type=int, default=1000)
    parser.add_argument("--no-temp-scaling", action="store_true")
    parser.add_argument("--head-id", help="Override head_id")
    args = parser.parse_args()

    db_path = args.db or default_db_path()
    if not db_path.exists():
        raise RuntimeError(f"DB not found: {db_path}")

    class_order = load_class_order(args.classes_json)
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row

    rows = load_training_rows(
        conn,
        args.model_id,
        args.min_confidence,
        args.ruleset_version,
        args.use_user_labels,
    )
    if not rows:
        raise RuntimeError("No labeled embeddings found for training.")

    X, y, class_ids = build_arrays(rows, class_order)
    if len(set(y)) < 2:
        raise RuntimeError("Need at least 2 classes to train.")

    train, val, test = split_stratified(
        X, y, seed=args.seed, test_fraction=args.test_fraction, val_fraction=args.val_fraction
    )

    model = train_logreg(
        train["X"],
        train["y"],
        l2=args.l2,
        class_weight=args.class_weight,
        max_iter=args.max_iter,
    )

    train_acc = print_metrics("train", model, train["X"], train["y"], class_ids)
    val_acc = print_metrics("val", model, val["X"], val["y"], class_ids)
    test_acc = print_metrics("test", model, test["X"], test["y"], class_ids)

    temperature = 1.0
    if not args.no_temp_scaling:
        temperature = fit_temperature(model, val["X"], val["y"])
        print(f"temperature: {temperature:.4f}")

    head_id = args.head_id or default_head_id()
    export_head(
        conn,
        head_id,
        args.model_id,
        class_ids,
        model,
        temperature,
    )
    created_at = int(dt.datetime.utcnow().timestamp())
    store_metrics(conn, head_id, "train", train_acc, None, None, None, created_at)
    store_metrics(conn, head_id, "val", val_acc, None, None, None, created_at)
    store_metrics(conn, head_id, "test", test_acc, None, None, None, created_at)
    log_gating_metrics(conn, head_id, "val", model, val["X"], val["y"], created_at)
    log_gating_metrics(conn, head_id, "test", model, test["X"], test["y"], created_at)
    print(f"Saved classifier head: {head_id}")
    return 0


def default_db_path() -> Path:
    config_home = os.environ.get("SEMPAL_CONFIG_HOME")
    if config_home:
        return Path(config_home) / ".sempal" / "library.db"
    if os.name == "nt":
        base = os.environ.get("APPDATA")
        if not base:
            raise RuntimeError("APPDATA is not set; pass --db explicitly.")
        return Path(base) / ".sempal" / "library.db"
    if sys_platform() == "darwin":
        return Path.home() / "Library" / "Application Support" / ".sempal" / "library.db"
    base = os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))
    return Path(base) / ".sempal" / "library.db"


def sys_platform() -> str:
    import platform

    return platform.system().lower()


def load_class_order(path: Path) -> list[str]:
    if not path.exists():
        raise RuntimeError(f"classes json not found: {path}")
    payload = json.loads(path.read_text())
    classes = payload.get("classes", [])
    order = [c["id"] for c in classes if c.get("id")]
    if not order:
        raise RuntimeError("classes json has no classes")
    return order


def load_training_rows(
    conn: sqlite3.Connection,
    model_id: str,
    min_confidence: float,
    ruleset_version: int,
    use_user_labels: bool,
) -> list[sqlite3.Row]:
    sql = """
    WITH best_weak AS (
        SELECT l.sample_id, l.class_id, l.confidence, l.rule_id, l.ruleset_version
        FROM labels_weak l
        WHERE l.ruleset_version = ?
          AND l.confidence >= ?
          AND l.class_id = (
            SELECT l2.class_id
            FROM labels_weak l2
            WHERE l2.sample_id = l.sample_id
              AND l2.ruleset_version = ?
              AND l2.confidence >= ?
            ORDER BY l2.confidence DESC, l2.class_id ASC
            LIMIT 1
          )
    )
    SELECT e.sample_id,
           e.vec,
           {class_expr} AS class_id
    FROM embeddings e
    {label_join}
    WHERE e.model_id = ?
      AND {label_filter}
    ORDER BY e.sample_id ASC
    """
    if use_user_labels:
        label_join = "LEFT JOIN labels_user u ON u.sample_id = e.sample_id LEFT JOIN best_weak w ON w.sample_id = e.sample_id"
        class_expr = "COALESCE(u.class_id, w.class_id)"
        label_filter = "u.class_id IS NOT NULL OR w.class_id IS NOT NULL"
    else:
        label_join = "JOIN best_weak w ON w.sample_id = e.sample_id"
        class_expr = "w.class_id"
        label_filter = "1=1"
    sql = sql.format(class_expr=class_expr, label_join=label_join, label_filter=label_filter)
    params = [ruleset_version, min_confidence, ruleset_version, min_confidence, model_id]
    return conn.execute(sql, params).fetchall()


def build_arrays(rows: list[sqlite3.Row], class_order: list[str]):
    try:
        import numpy as np
    except Exception as err:
        raise RuntimeError("numpy is required (pip install numpy)") from err

    class_set = {row["class_id"] for row in rows if row["class_id"]}
    class_ids = [cid for cid in class_order if cid in class_set]
    if not class_ids:
        raise RuntimeError("No matching class_ids in training data.")
    class_to_idx = {cid: i for i, cid in enumerate(class_ids)}

    X = []
    y = []
    for row in rows:
        class_id = row["class_id"]
        if class_id not in class_to_idx:
            continue
        vec = np.frombuffer(row["vec"], dtype="<f4")
        X.append(vec)
        y.append(class_to_idx[class_id])

    if not X:
        raise RuntimeError("No embeddings after filtering classes.")
    X = np.stack(X, axis=0)
    y = np.asarray(y, dtype=np.int64)
    return X, y, class_ids


def split_stratified(X, y, seed: int, test_fraction: float, val_fraction: float):
    try:
        from sklearn.model_selection import train_test_split
    except Exception as err:
        raise RuntimeError("scikit-learn is required (pip install scikit-learn)") from err

    if test_fraction < 0 or val_fraction < 0 or (test_fraction + val_fraction) >= 1.0:
        raise RuntimeError("Invalid test/val fractions")

    X_train, X_test, y_train, y_test = train_test_split(
        X, y, test_size=test_fraction, random_state=seed, stratify=y
    )
    val_ratio = val_fraction / max(1e-6, 1.0 - test_fraction)
    X_train, X_val, y_train, y_val = train_test_split(
        X_train, y_train, test_size=val_ratio, random_state=seed, stratify=y_train
    )
    return (
        {"X": X_train, "y": y_train},
        {"X": X_val, "y": y_val},
        {"X": X_test, "y": y_test},
    )


def train_logreg(X, y, l2: float, class_weight: str, max_iter: int):
    try:
        from sklearn.linear_model import LogisticRegression
    except Exception as err:
        raise RuntimeError("scikit-learn is required (pip install scikit-learn)") from err

    if l2 <= 0:
        raise RuntimeError("l2 must be > 0")
    C = 1.0 / l2
    weight = None if class_weight == "none" else "balanced"
    model = LogisticRegression(
        multi_class="multinomial",
        solver="lbfgs",
        max_iter=max_iter,
        C=C,
        class_weight=weight,
        n_jobs=1,
    )
    model.fit(X, y)
    return model


def print_metrics(name: str, model, X, y, class_ids: list[str]):
    try:
        import numpy as np
        from sklearn.metrics import accuracy_score, confusion_matrix, recall_score
    except Exception as err:
        raise RuntimeError("scikit-learn and numpy are required") from err

    preds = model.predict(X)
    acc = accuracy_score(y, preds)
    cm = confusion_matrix(y, preds, labels=list(range(len(class_ids))))
    recall = recall_score(y, preds, labels=list(range(len(class_ids))), average=None)
    print(f"{name} accuracy: {acc:.4f}")
    print(f"{name} confusion matrix:")
    print(cm)
    print(f"{name} per-class recall:")
    for idx, cid in enumerate(class_ids):
        print(f"  {cid}: {recall[idx]:.4f}")
    return float(acc)


def log_gating_metrics(conn, head_id, split, model, X, y, created_at: int):
    try:
        import numpy as np
    except Exception as err:
        raise RuntimeError("numpy is required (pip install numpy)") from err

    probs = model.predict_proba(X)
    if probs.ndim == 1:
        probs = np.stack([1.0 - probs, probs], axis=1)
    sorted_probs = np.sort(probs, axis=1)[:, ::-1]
    top1 = sorted_probs[:, 0]
    top2 = sorted_probs[:, 1] if sorted_probs.shape[1] > 1 else np.zeros_like(top1)
    margins = top1 - top2
    preds = probs.argmax(axis=1)
    correct = preds == y
    thresholds = [0.02, 0.05, 0.1, 0.15, 0.2]
    print(f"{split} gating metrics:")
    for t in thresholds:
        covered = margins >= t
        coverage = float(covered.mean())
        precision = float(correct[covered].mean()) if covered.any() else None
        store_metrics(conn, head_id, split, None, coverage, precision, t, created_at)
        if precision is None:
            print(f"  margin>={t:.2f}: coverage={coverage:.3f}, precision=NA")
        else:
            print(f"  margin>={t:.2f}: coverage={coverage:.3f}, precision={precision:.3f}")


def store_metrics(
    conn,
    head_id: str,
    split: str,
    accuracy,
    coverage,
    precision,
    threshold,
    created_at: int,
):
    cur = conn.cursor()
    cur.execute(
        "INSERT INTO classifier_metrics (head_id, split, accuracy, coverage, precision, threshold, created_at)\n"
        "VALUES (?, ?, ?, ?, ?, ?, ?)\n"
        "ON CONFLICT(head_id, split, threshold) DO UPDATE SET\n"
        "  accuracy = excluded.accuracy,\n"
        "  coverage = excluded.coverage,\n"
        "  precision = excluded.precision,\n"
        "  created_at = excluded.created_at",
        (head_id, split, accuracy, coverage, precision, threshold, created_at),
    )
    conn.commit()


def fit_temperature(model, X_val, y_val) -> float:
    try:
        import numpy as np
    except Exception as err:
        raise RuntimeError("numpy is required (pip install numpy)") from err

    logits = model.decision_function(X_val)
    if logits.ndim == 1:
        logits = np.stack([-logits, logits], axis=1)

    def nll(temp: float) -> float:
        temp = max(1e-3, float(temp))
        scaled = logits / temp
        scaled = scaled - scaled.max(axis=1, keepdims=True)
        exp = np.exp(scaled)
        probs = exp / exp.sum(axis=1, keepdims=True)
        idx = np.arange(len(y_val))
        return -np.log(probs[idx, y_val] + 1e-12).mean()

    best_t = 1.0
    best_loss = nll(best_t)
    try:
        from scipy.optimize import minimize

        res = minimize(lambda x: nll(x[0]), x0=[1.0], bounds=[(0.05, 10.0)])
        if res.success:
            best_t = float(res.x[0])
            best_loss = nll(best_t)
    except Exception:
        for temp in np.logspace(-1.3, 0.7, num=40):
            loss = nll(temp)
            if loss < best_loss:
                best_loss = loss
                best_t = float(temp)
    return best_t


def default_head_id() -> str:
    date = dt.datetime.utcnow().strftime("%Y%m%d")
    sha = os.environ.get("SEMPAL_GIT_SHA", "nogit")
    return f"softmax_lr__{date}__{sha}"


def export_head(conn, head_id, model_id, class_ids, model, temperature: float):
    import numpy as np

    weights = model.coef_.astype(np.float32)
    bias = model.intercept_.astype(np.float32)
    if weights.ndim != 2:
        raise RuntimeError("Unexpected weights shape")
    dim = weights.shape[1]
    num_classes = weights.shape[0]
    if bias.shape[0] != num_classes:
        raise RuntimeError("Bias length mismatch")
    if num_classes != len(class_ids):
        raise RuntimeError("class_ids length mismatch")

    weights_blob = weights.reshape(-1).tobytes(order="C")
    bias_blob = bias.tobytes(order="C")
    cur = conn.cursor()
    cur.execute(
        "INSERT INTO classifier_models (head_id, model_id, dim, num_classes, norm, temperature, weights, bias)\n"
        "VALUES (?, ?, ?, ?, ?, ?, ?, ?)\n"
        "ON CONFLICT(head_id) DO UPDATE SET\n"
        "  model_id = excluded.model_id,\n"
        "  dim = excluded.dim,\n"
        "  num_classes = excluded.num_classes,\n"
        "  norm = excluded.norm,\n"
        "  temperature = excluded.temperature,\n"
        "  weights = excluded.weights,\n"
        "  bias = excluded.bias",
        (
            head_id,
            model_id,
            int(dim),
            int(num_classes),
            "l2",
            float(temperature),
            sqlite3.Binary(weights_blob),
            sqlite3.Binary(bias_blob),
        ),
    )
    conn.commit()


if __name__ == "__main__":
    raise SystemExit(main())
