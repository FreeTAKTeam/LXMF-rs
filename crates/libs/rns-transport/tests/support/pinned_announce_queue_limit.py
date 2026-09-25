import ast
import subprocess
import sys

repo, expected_revision = sys.argv[1:]
subprocess.check_call(["git", "-C", repo, "cat-file", "-e", f"{expected_revision}^{{commit}}"])
source = subprocess.check_output(
    ["git", "-C", repo, "show", f"{expected_revision}:RNS/Reticulum.py"], text=True
)
tree = ast.parse(source)
reticulum = next(
    node for node in tree.body
    if isinstance(node, ast.ClassDef) and node.name == "Reticulum"
)
capacity = next(
    node.value.value
    for node in reticulum.body
    if isinstance(node, ast.Assign)
    and any(isinstance(target, ast.Name) and target.id == "MAX_QUEUED_ANNOUNCES" for target in node.targets)
)
assert isinstance(capacity, int) and capacity > 0
print(capacity)
