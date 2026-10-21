import { expect, test } from "vitest";
import { validate, type Draft } from "./validate";

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
) => ({ type: "Local", bind_host, bind_port, dest_port }) as const;

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
