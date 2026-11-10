import { expect, test } from "vitest";
import { newForward, toRule, validate, type Draft } from "./validate";

const ok: Draft = {
  name: "a",
  host: "h",
  port: 22,
  username: "u",
  forwards: [],
};
const local = (
  bind_host: string,
  bind_port: number | null = 80,
  dest_port: number | null = 80,
) =>
  ({
    type: "Local",
    bind_host,
    bind_port,
    dest_host: "db",
    dest_port,
    auto_start: false,
  }) as const;

test("mirrors the core rules", () => {
  expect(validate(ok)).toEqual({});
  expect(
    Object.keys(
      validate({ ...ok, name: " ", host: "", username: "\t", port: 0 }),
    ),
  ).toEqual(["name", "host", "username", "port"]);
  expect(validate({ ...ok, port: 65536 }).port).toBeDefined();
  expect(validate({ ...ok, port: null }).port).toBeDefined();
});

test("local forwards need an IP, remote ones skip it", () => {
  for (const good of ["127.0.0.1", "0.0.0.0", "::1", "fe80::1"])
    expect(validate({ ...ok, forwards: [local(good)] })).toEqual({});
  for (const bad of ["localhost", "256.1.1.1", "01.1.1.1", "", "1.2.3"])
    expect(
      validate({ ...ok, forwards: [local(bad)] })["forwards[0].bind_host"],
    ).toBeDefined();
  const remote = { ...local(""), type: "Remote" } as const;
  expect(validate({ ...ok, forwards: [remote] })).toEqual({});
  expect(
    Object.keys(validate({ ...ok, forwards: [local("::1", 0, null)] })),
  ).toEqual(["forwards[0].bind_port", "forwards[0].dest_port"]);
});

test("dynamic forwards need an IP and a port but no destination", () => {
  const dyn = (bind_host: string, bind_port: number | null) =>
    ({
      type: "Dynamic",
      bind_host,
      bind_port,
      dest_host: "",
      dest_port: null,
      auto_start: false,
    }) as const;
  expect(validate({ ...ok, forwards: [dyn("127.0.0.1", 1080)] })).toEqual({});
  expect(
    Object.keys(validate({ ...ok, forwards: [dyn("localhost", null)] })),
  ).toEqual(["forwards[0].bind_host", "forwards[0].bind_port"]);
});

test("local and remote forwards need a destination host", () => {
  for (const type of ["Local", "Remote"] as const)
    expect(
      Object.keys(
        validate({
          ...ok,
          forwards: [{ ...local("::1"), type, dest_host: " " }],
        }),
      ),
    ).toEqual(["forwards[0].dest_host"]);
});

test("a new forward becomes the rule it stands for", () => {
  expect(newForward("Dynamic")).toMatchObject({
    bind_host: "127.0.0.1",
    bind_port: 1080,
  });
  expect(newForward("Remote").bind_host).toBe("");
  const draft = { ...local(" ::1 ", 8080, 5432), dest_host: " db " };
  expect(toRule(draft)).toEqual({
    type: "Local",
    bind_host: "::1",
    bind_port: 8080,
    dest_host: "db",
    dest_port: 5432,
    auto_start: false,
  });
  expect(toRule({ ...draft, type: "Remote" })).toEqual({
    type: "Remote",
    bind_port: 8080,
    dest_host: "db",
    dest_port: 5432,
    auto_start: false,
  });
  expect(toRule({ ...draft, type: "Dynamic" })).toEqual({
    type: "Dynamic",
    bind_host: "::1",
    bind_port: 8080,
    auto_start: false,
  });
});
