use bevy::color::{palettes::css, LinearRgba};
use bevy::math::primitives::Sphere;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use std::f32::consts::PI;

/// Resource: simulation-wide tunables available to any system.
/// In ECS, Resources are global singletons that systems can read/mutate.
#[derive(Resource)]
struct SimulationParams {
    orbit_speed_scale: f32,    // scales all orbital angular speeds
    spin_speed_scale: f32,     // scales all self-rotation angular speeds
    earth_orbit_radius: f32,   // Sun–Earth distance
    moon_orbit_radius: f32,    // Earth–Moon distance
    use_elliptical_orbit: bool // when true, Earth uses parametric ellipse instead of pivot rotation
}

/// Component: entity whose local yaw is rotated each frame to carry children
/// in a circular path (we attach this to *pivot* entities).
#[derive(Component)]
struct Orbit {
    angular_speed: f32, // rad/s (base), multiplied by SimulationParams.orbit_speed_scale
}

/// Component: entity spins around its own local Y axis each frame.
#[derive(Component)]
struct Spin {
    angular_speed: f32, // rad/s (base), multiplied by SimulationParams.spin_speed_scale
}

/// Tags for clarity / filtering.
#[derive(Component)] struct Sun;
#[derive(Component)] struct Earth;
#[derive(Component)] struct Moon;

/// Component: simple parametric elliptical orbit for an entity (e.g., Earth).
/// We integrate an explicit parameter angle `theta` over time (not true anomaly).
#[derive(Component)]
struct EllipticalOrbit {
    a: f32,             // semi-major axis
    b: f32,             // semi-minor axis
    angular_speed: f32, // parametric speed (rad/s)
    theta: f32,         // current param angle (state)
}

fn main() {
    App::new()
        // Core plugins + UI plugin
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Earth orbiting the Sun (Bevy)".into(),
                    ..default()
                }),
                ..default()
            }),
            EguiPlugin,
        ))
        // A dim ambient so the Sun's light + PBR shading stand out
        .insert_resource(AmbientLight {
            color: Color::srgb(0.1, 0.1, 0.2),
            brightness: 0.5,
        })
        // Default simulation parameters
        .insert_resource(SimulationParams {
            orbit_speed_scale: 1.0,
            spin_speed_scale: 1.0,
            earth_orbit_radius: 3.0,
            moon_orbit_radius: 0.9,
            use_elliptical_orbit: false,
        })
        // Build the initial ECS world (entities/graph)
        .add_systems(Startup, setup)
        // Behavior systems run every frame (Update schedule)
        .add_systems(
            Update,
            (
                animate_orbits,            // rotate pivots for circular orbits
                spin_bodies,               // spin Sun/Earth/Moon
                animate_elliptical_orbits, // drive Earth along an ellipse if enabled
                enforce_orbit_radii,       // apply new radii from sliders in circular mode
                ui_panel,                  // sliders UI
            ),
        )
        .run();
}

/// Startup system: spawns camera, light, Sun, Earth (with tilt), Moon, and their pivots.
/// Uses parent-child hierarchy to express spatial relationships.
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    params: Res<SimulationParams>,
) {
    // Camera looking at world origin
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(-6.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..default()
    });

    // Directional light to mimic sunlight (parallel rays, strong illuminance)
    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            shadows_enabled: true,
            illuminance: 60_000.0,
            ..default()
        },
        transform: Transform::from_rotation(Quat::from_euler(
            EulerRot::XYZ,
            -PI / 4.0,
            -PI / 4.0,
            0.0,
        )),
        ..default()
    });

    // Sun: emissive PBR sphere + a gentle spin (purely visual)
    let sun_mesh = meshes.add(Mesh::from(Sphere { radius: 1.0 }));
    let sun_mat = materials.add(StandardMaterial {
        base_color: css::ORANGE.into(),
        // In Bevy 0.14, emissive is LinearRgba. Use a scaled linear color for a brighter "glow".
        emissive: LinearRgba::from(css::ORANGE) * 5.0,
        ..default()
    });
    let sun = commands
        .spawn((
            PbrBundle {
                mesh: sun_mesh,
                material: sun_mat,
                transform: Transform::from_translation(Vec3::ZERO),
                ..default()
            },
            Sun,
            Spin {
                angular_speed: 0.2,
            },
        ))
        .id();

    // Earth pivot: rotates to carry the Earth around the Sun in a circle
    let earth_pivot = commands
        .spawn((
            SpatialBundle::from_transform(Transform::from_translation(Vec3::ZERO)),
            Orbit {
                angular_speed: PI / 10.0, // ~1 revolution in ~20s before scaling
            },
        ))
        .id();

    // Earth: tilted axis (~23.44°), initially placed along +X at orbit radius
    let earth_mesh = meshes.add(Mesh::from(Sphere { radius: 0.5 }));
    let earth_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.4, 1.0),
        ..default()
    });
    let tilt = 23.44_f32.to_radians();
    let earth = commands
        .spawn((
            PbrBundle {
                mesh: earth_mesh,
                material: earth_mat,
                transform: Transform::from_xyz(params.earth_orbit_radius, 0.0, 0.0)
                    .with_rotation(Quat::from_axis_angle(Vec3::Z, tilt)),
                ..default()
            },
            Earth,
            Spin {
                angular_speed: PI * 2.0, // ~1 self-rotation per second before scaling
            },
            // Uncomment to start in elliptical mode with preset a/b and param speed:
            // EllipticalOrbit { a: 3.2, b: 2.6, angular_speed: PI / 10.0, theta: 0.0 },
        ))
        .id();

    // Moon pivot: child of Earth (so it follows Earth around the Sun)
    let moon_pivot = commands
        .spawn((
            SpatialBundle::default(),
            Orbit {
                angular_speed: PI * 3.0, // faster orbit around Earth
            },
        ))
        .id();

    // Moon: smaller sphere offset along +X in Earth's local space
    let moon_mesh = meshes.add(Mesh::from(Sphere { radius: 0.18 }));
    let moon_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.8, 0.8),
        ..default()
    });
    let moon = commands
        .spawn((
            PbrBundle {
                mesh: moon_mesh,
                material: moon_mat,
                transform: Transform::from_xyz(params.moon_orbit_radius, 0.0, 0.0),
                ..default()
            },
            Moon,
            Spin {
                angular_speed: PI * 0.3,
            },
        ))
        .id();

    // Build hierarchy:
    // Sun
    //  └─ earth_pivot (rotates: carries Earth around)
    //      └─ Earth
    //          └─ moon_pivot (rotates: carries Moon around Earth)
    //              └─ Moon
    commands.entity(moon_pivot).push_children(&[moon]);
    commands.entity(earth).push_children(&[moon_pivot]);
    commands.entity(earth_pivot).push_children(&[earth]);
    commands.entity(sun).push_children(&[earth_pivot]);
}

/// System: rotates any entity with `Orbit` around its local Y axis.
/// In this scene, these are *pivot* entities; children inherit the motion via hierarchy.
fn animate_orbits(mut q: Query<(&Orbit, &mut Transform)>, time: Res<Time>, params: Res<SimulationParams>) {
    let dt = time.delta_seconds();
    for (orbit, mut transform) in &mut q {
        transform.rotate_y(orbit.angular_speed * params.orbit_speed_scale * dt);
    }
}

/// System: rotates entities with `Spin` around their local Y axis.
/// Independent from orbital motion; purely visual for Sun/Earth/Moon.
fn spin_bodies(mut q: Query<(&Spin, &mut Transform)>, time: Res<Time>, params: Res<SimulationParams>) {
    let dt = time.delta_seconds();
    for (spin, mut transform) in &mut q {
        transform.rotate_local_y(spin.angular_speed * params.spin_speed_scale * dt);
    }
}

/// System: drives `EllipticalOrbit` bodies by directly setting their translation.
/// Attach to Earth if `use_elliptical_orbit` is true. This overrides parent-pivot rotation.
fn animate_elliptical_orbits(
    mut q: Query<(&mut Transform, &mut EllipticalOrbit), With<Earth>>,
    time: Res<Time>,
    params: Res<SimulationParams>,
) {
    if !params.use_elliptical_orbit {
        return;
    }
    let dt = time.delta_seconds();
    for (mut transform, mut e) in &mut q {
        e.theta += e.angular_speed * params.orbit_speed_scale * dt;
        let x = e.a * e.theta.cos();
        let z = e.b * e.theta.sin();
        transform.translation = Vec3::new(x, 0.0, z);
    }
}

/// System: when using circular mode, enforce new orbit radii from sliders
/// by resetting the local translations of Earth and Moon.
/// (In elliptical mode, position is driven by `animate_elliptical_orbits`.)
fn enforce_orbit_radii(
    params: Res<SimulationParams>,
    mut earth_q: Query<&mut Transform, (With<Earth>, Without<Moon>)>,
    mut moon_q: Query<&mut Transform, (With<Moon>, Without<Earth>)>,
) {
    if params.is_changed() && !params.use_elliptical_orbit {
        if let Ok(mut t) = earth_q.get_single_mut() {
            t.translation = Vec3::new(params.earth_orbit_radius, 0.0, 0.0);
        }
        if let Ok(mut t) = moon_q.get_single_mut() {
            t.translation = Vec3::new(params.moon_orbit_radius, 0.0, 0.0);
        }
    }
}

/// UI system: exposes sliders to tweak the simulation at runtime.
/// In ECS terms, this system mutates the global `SimulationParams` Resource.
fn ui_panel(mut contexts: EguiContexts, mut params: ResMut<SimulationParams>) {
    egui::Window::new("Simulation").show(contexts.ctx_mut(), |ui| {
        ui.heading("Speeds & scales");
        ui.add(egui::Slider::new(&mut params.orbit_speed_scale, 0.0..=5.0).text("Orbit speed ×"));
        ui.add(egui::Slider::new(&mut params.spin_speed_scale, 0.0..=5.0).text("Spin speed ×"));

        ui.separator();
        ui.heading("Distances");
        ui.add(egui::Slider::new(&mut params.earth_orbit_radius, 1.0..=10.0).text("Earth radius"));
        ui.add(egui::Slider::new(&mut params.moon_orbit_radius, 0.2..=3.0).text("Moon radius"));

        ui.separator();
        ui.checkbox(&mut params.use_elliptical_orbit, "Use elliptical orbit for Earth");
        ui.label("Ellipse uses x = a cos(θ), z = b sin(θ). For simplicity, timing is parametric.");

        ui.separator();
        ui.label("Tip: for elliptical mode, attach `EllipticalOrbit` to Earth in `setup()`.");
    });
}
