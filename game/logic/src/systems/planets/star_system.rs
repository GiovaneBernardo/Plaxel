use engine::{
    assets::material::Material,
    core::components::core::TransformComponent,
    ecs::{commands::Commands, entity::Entity, system::SystemContext},
    math::{Mat4, Quat, Vec3, vec3},
    model::{ModelVertex, TransformInstance, Vertex},
    renderer::{CullMode, RenderObject, RenderObjectId, material_passes},
};
use game_types::universe::StarSystemComponent;

use crate::systems::planet_system;

pub fn create_star_system(ctx: &mut SystemContext, commands: &mut Commands) {
    let world = &mut ctx.world;

    let star_entity = world.spawn();
    world.insert(
        star_entity,
        StarSystemComponent {
            planets: Vec::new(),
            mass: 1e9,
            radius: 1e5,
            surface_temperature: 5000.0,
            luminosity: luminosity(1e5, 5000.0),
            emission_color: star_color(5000.0),
        },
    );

    world.insert(
        star_entity,
        TransformComponent {
            position: vec3(149.0 * 1_000_000_000.0, 1e6, 0.0),
            rotation: Quat::IDENTITY,
            scale: vec3(1e5, 1e5, 1e5),
            velocity: vec3(0.0, 0.0, 0.0),
        },
    );

    create_star_render_object(ctx, star_entity);

    // Create planets
    for i in 0..1 {
        let position = if i == 0 {
            Some(vec3(0.0, 0.0, 0.0))
        } else {
            None
        };

        let planet = planet_system::create_planet(ctx, commands, star_entity, position);

        if planet.is_some() {
            ctx.world
                .get_mut::<StarSystemComponent>(star_entity)
                .unwrap()
                .planets
                .push(planet.unwrap());
        }
    }
}

const SIGMA: f64 = 5.670_374_419e-8;

fn luminosity(radius_m: f64, temperature_k: f64) -> f64 {
    4.0 * std::f64::consts::PI * radius_m.powi(2) * SIGMA * temperature_k.powi(4)
}

fn star_color(temperature: f64) -> Vec3 {
    if temperature < 3000.0 {
        return vec3(255.0 / 255.0, 180.0 / 255.0, 107.0 / 255.0);
    } else if temperature < 4500.0 {
        return vec3(255.0 / 255.0, 219.0 / 255.0, 186.0 / 255.0);
    } else if temperature < 5772.0 {
        return vec3(255.0 / 255.0, 243.0 / 255.0, 239.0 / 255.0);
    } else if temperature < 8000.0 {
        return vec3(221.0 / 255.0, 229.0 / 255.0, 255.0 / 255.0);
    } else if temperature < 12000.0 {
        return vec3(191.0 / 255.0, 211.0 / 255.0, 255.0 / 255.0);
    } else if temperature < 30000.0 {
        return vec3(159.0 / 255.0, 190.0 / 255.0, 255.0 / 255.0);
    }
    vec3(0.0, 0.0, 0.0)
}

fn create_star_render_object(ctx: &mut SystemContext, star_entity: Entity) -> RenderObjectId {
    let transform = ctx.world.get::<TransformComponent>(star_entity).unwrap();
    let model_matrix = Mat4::from_translation(transform.position)
        * Mat4::from_quat(transform.rotation)
        * Mat4::from_scale(Vec3::new(
            transform.scale.x,
            transform.scale.y,
            transform.scale.z,
        ));

    let mut star_material =
        Material::for_pass("shaders/star.wgsl".into(), material_passes::FORWARD_OPAQUE)
            .with_vertex_layouts(vec![ModelVertex::layout(), TransformInstance::layout()])
            .with_cull(CullMode::Front);

    star_material.material_index = ctx
        .globals
        .renderer
        .renderer_api
        .upload_material_asset(&star_material, None);

    let transform = engine::model::TransformInstance {
        model_matrix: model_matrix.to_cols_array_2d(),
        material_index: star_material.material_index,
    };

    let mesh = ctx.globals.renderer.default_meshes().sphere;
    let object_id: RenderObjectId =
        ctx.globals
            .renderer
            .objects()
            .insert(RenderObject::new(mesh, star_material, transform));

    return object_id;
}
