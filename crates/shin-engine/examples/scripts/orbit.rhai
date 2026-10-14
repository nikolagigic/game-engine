// Orbit script for the `scripted_orbit` example (GE-011).
//
// Edit this file while the example runs: changes take effect within a
// second, without restarting. Try `let speed = -2.0;` or `let bob = 1.0;`.

fn update(entity, dt) {
    let speed = 0.8; // radians per second around the Y axis
    let bob = 0.3; // height of the up-and-down bob

    let t = entity.transform;
    let p = t.translation;

    // Turn the position about the Y axis by speed * dt. Each cube keeps
    // its own radius and phase, so one script drives every cube.
    let a = speed * dt;
    let x = p.x * cos(a) + p.z * sin(a);
    let z = p.z * cos(a) - p.x * sin(a);

    // Bob up and down, each cube offset by where it is on the circle.
    let y = bob * sin(time() * 2.0 + atan(z, x));

    t.translation = vec3(x, y, z);
    t.rotation = quat_euler(0.0, time(), 0.0);
}
