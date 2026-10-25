import { expect, test } from "vitest";
import { colorOf, initialsOf } from "./appearance";
import type { Group } from "./bindings/Group";

const g: Group = { id: "g1", name: "prod" };

test("colorOf prefers the connection color, then the group's, else null", () => {
  expect(
    colorOf({ color: "red", group_id: g.id }, [{ ...g, color: "teal" }]),
  ).toBe("red");
  expect(colorOf({ group_id: g.id }, [{ ...g, color: "teal" }])).toBe("teal");
  expect(colorOf({ group_id: g.id }, [g])).toBeNull();
  expect(colorOf({ group_id: null }, [])).toBeNull();
  expect(colorOf({ group_id: "gone" }, [])).toBeNull();
});

test("initialsOf", () => {
  expect(initialsOf("prod-db")).toBe("PD");
  expect(initialsOf("nas")).toBe("NA");
  expect(initialsOf("my web.server_1")).toBe("MW");
  expect(initialsOf("x")).toBe("X");
  expect(initialsOf("città")).toBe("CI");
  expect(initialsOf("--")).toBe("?");
});
