import { expect, test } from "vitest";
import type { Candidate } from "./bindings/Candidate";
import { brokenJumps, prepare, requiredBy, toggle } from "./importSelection";

const cand = (
  id: string,
  jump_via: string | null = null,
  username = "u",
  extra: Partial<Candidate> = {},
): Candidate => ({
  connection: {
    id,
    name: id,
    group_id: null,
    host: `${id}.example`,
    port: 22,
    username,
    auth: { type: "Auto" },
    jump_via,
    forwards: [],
  },
  notes: [],
  error: null,
  exists: false,
  needs_user: username === "",
  ...extra,
});

test("selecting a host selects its jump host", () => {
  const cs = [cand("a", "b"), cand("b", "c"), cand("c"), cand("d")];
  expect([...toggle(new Set(), cs, "a")].sort()).toEqual(["a", "b", "c"]);
});

test("selecting does not pull in a jump host that cannot be imported", () => {
  const cs = [cand("a", "b"), cand("b", null, "u", { exists: true })];
  expect([...toggle(new Set(), cs, "a")]).toEqual(["a"]);
});

test("unselecting leaves the other hosts alone", () => {
  const cs = [cand("a", "b"), cand("b")];
  expect([...toggle(new Set(["a", "b"]), cs, "b")]).toEqual(["a"]);
});

test("requiredBy lists the selected hosts using a jump host", () => {
  const cs = [cand("a", "b"), cand("c", "b"), cand("d", "b")];
  expect(requiredBy(cs, new Set(["a", "b", "c"]), "b")).toEqual(["a", "c"]);
});

test("unselecting the jump host clears jump_via on the hosts using it", () => {
  const cs = [cand("a", "b"), cand("b")];
  const out = prepare(cs, new Set(["a"]), "me");
  expect(out.map((c) => c.id)).toEqual(["a"]);
  expect(out[0]?.jump_via).toBeNull();
});

test("a jump to a saved connection is kept", () => {
  const out = prepare([cand("a", "saved-id")], new Set(["a"]), "me");
  expect(out[0]?.jump_via).toBe("saved-id");
});

test("missing user is filled from the shared field", () => {
  const out = prepare(
    [cand("a", null, ""), cand("b")],
    new Set(["a", "b"]),
    "me",
  );
  expect(out.map((c) => c.username)).toEqual(["me", "u"]);
});

test("a jump cycle is broken so the core accepts the set", () => {
  const out = prepare(
    [cand("a", "b"), cand("b", "a")],
    new Set(["a", "b"]),
    "",
  );
  expect(out.filter((c) => c.jump_via === null).length).toBeGreaterThan(0);
  const [a, b] = out;
  expect(a?.jump_via === "b" && b?.jump_via === "a").toBe(false);
});

test("a chain deeper than 5 hops is cut", () => {
  const ids = ["n0", "n1", "n2", "n3", "n4", "n5", "n6", "n7"];
  const cs = ids.map((id, i) => cand(id, ids[i + 1] ?? null));
  const out = prepare(cs, new Set(ids), "");
  const via = new Map(out.map((c) => [c.id, c.jump_via]));
  for (const c of out) {
    let hops = 0;
    for (let n = c.jump_via; n; n = via.get(n) ?? null) hops++;
    expect(hops).toBeLessThanOrEqual(5);
  }
  expect(via.get("n7")).toBeNull();
  expect(out.some((c) => c.jump_via)).toBe(true);
});

test("brokenJumps names exactly the hosts whose jump prepare clears", () => {
  const cs = [cand("a", "b"), cand("b", "a"), cand("c", "d"), cand("d")];
  const sel = new Set(["a", "b", "c", "d"]);
  const cleared = prepare(cs, sel, "")
    .filter(
      (c) =>
        c.jump_via === null &&
        cs.find((x) => x.connection.id === c.id)?.connection.jump_via,
    )
    .map((c) => c.id);
  expect([...brokenJumps(cs, sel)]).toEqual(cleared);
  expect(cleared.length).toBe(1);
});

test("brokenJumps ignores jump hosts that are simply not imported", () => {
  const cs = [cand("a", "b"), cand("b")];
  expect(brokenJumps(cs, new Set(["a"])).size).toBe(0);
});
