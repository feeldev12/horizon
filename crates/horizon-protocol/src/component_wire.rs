//! The wire layout of the data components Horizon keeps without interpreting (Nerion): every
//! network-synchronized component of 26.3 that `item::Component` has no typed form for. A data
//! component has no length prefix, so one Horizon can't walk makes the whole stack, and the packet
//! carrying it, undecodable: player heads, written books, potions, fireworks, banners, shulker
//! boxes and attribute modifiers did, all over real servers. Each reader here follows the
//! component's STREAM_CODEC in 26.3 (DataComponents and the codecs it names), consuming exactly
//! its bytes; `item::Component::Raw` keeps them, so the stack is sent back unchanged.
use crate::{Error, Result, buf::Reader, item};

/// Walks one component of type `kind` (minecraft:data_component_type id). `None` for the types
/// `item::Component` reads itself or that are never sent.
pub fn skip(kind: i32, r: &mut Reader) -> Option<Result<()>> {
    Some(match kind {
        // use_effects: can_sprint, interact_vibrations, speed_multiplier
        5 => all(&[bool_, bool_, f32_], r),
        // minimum_attack_charge, potion_duration_scale
        7 | 54 => f32_(r),
        // damage_type, rarity-like registry ids and enums: block_transformer, villager_food,
        // dye, map_post_processing, additional_trade_cost, ominous_bottle_amplifier, enchantable,
        // base_color and every entity variant/colour (villager/variant .. shulker/color,
        // provides_pottery_pattern, cushion/color)
        8 | 31 | 42 | 43 | 44 | 46 | 50 | 65 | 75 | 88..=108 | 110..=117 | 121 => varint(r),
        // can_place_on, can_break: AdventureModePredicate = list of BlockPredicate
        14 | 15 => list(r, block_predicate),
        16 => list(r, attribute_modifier_entry),
        // food: nutrition, saturation, can_always_eat
        23 => all(&[varint, f32_, bool_], r),
        // consumable: seconds, animation, sound, particles, on-consume effects
        24 => (|| {
            f32_(r)?;
            varint(r)?;
            sound_event(r)?;
            bool_(r)?;
            list(r, consume_effect)
        })(),
        // use_remainder, sulfur_cube_content
        25 | 80 => item_template(r),
        // use_cooldown: seconds, optional group
        26 => f32_(r).and_then(|_| optional(r, ident)),
        // damage_resistant, repairable, provides_banner_patterns
        27 | 33 | 67 => holder_set(r),
        28 => tool(r),
        // weapon: item damage per attack, disable blocking seconds
        29 => all(&[varint, f32_], r),
        // attack_range: six floats
        30 => all(&[f32_, f32_, f32_, f32_, f32_, f32_], r),
        32 => equippable(r),
        // glider, waxed: Unit
        34 | 120 => Ok(()),
        // tooltip_style, note_block_sound: Identifier
        35 | 73 => ident(r),
        // death_protection: list of consume effects
        36 => list(r, consume_effect),
        37 => blocks_attacks(r),
        // piercing_weapon: deals knockback, dismounts, sound, hit sound
        38 => (|| {
            bool_(r)?;
            bool_(r)?;
            optional(r, sound_event)?;
            optional(r, sound_event)
        })(),
        39 => kinetic_weapon(r),
        // attack_animation, interact_animation: SwingAnimation (type, duration)
        40 | 41 => all(&[varint, varint], r),
        // charged_projectiles, bundle_contents
        51 | 52 => list(r, item_template),
        53 => potion_contents(r),
        // suspicious_stew_effects: (effect, duration)
        55 => list(r, |r| all(&[varint, varint], r)),
        // writable_book_content: pages as Filterable<String>
        56 => list(r, |r| filterable(r, string)),
        57 => written_book_content(r),
        // trim: material and pattern holders
        58 => holder(r, |r| ident(r).and_then(|_| nbt(r)))
            .and_then(|_| holder(r, |r| all(&[ident, nbt, bool_], r))),
        // entity_data, block_entity_data: TypedEntityData (type id, compound)
        60 | 62 => typed_entity_data(r),
        // bucket_entity_data: CustomData compound
        61 => nbt(r),
        // instrument: Holder<Instrument>
        63 => holder(r, |r| {
            sound_event(r)?;
            all(&[f32_, f32_, varint, nbt], r)
        }),
        // provides_trim_material: Holder<TrimMaterial>
        64 => holder(r, |r| ident(r).and_then(|_| nbt(r))),
        // jukebox_playable: Holder<JukeboxSong>
        66 => holder(r, |r| {
            sound_event(r)?;
            all(&[nbt, f32_, varint], r)
        }),
        // lodestone_tracker: optional GlobalPos (dimension, packed block pos), tracked
        69 => optional(r, |r| ident(r).and_then(|_| i64_(r))).and_then(|_| bool_(r)),
        70 => firework_explosion(r),
        // fireworks: flight duration, explosions
        71 => varint(r).and_then(|_| list(r, firework_explosion)),
        72 => resolvable_profile(r),
        // banner_patterns: layers of (Holder<BannerPattern>, DyeColor)
        74 => list(r, |r| {
            holder(r, |r| ident(r).and_then(|_| string(r)))?;
            varint(r)
        }),
        // pot_decorations: back, left, right, front
        76 => (0..4).try_for_each(|_| optional(r, item_template)),
        // container: up to 256 optional stacks
        77 => list(r, |r| optional(r, item_template)),
        // block_state: string -> string map
        78 => list(r, |r| all(&[string, string], r)),
        // bees: (TypedEntityData, ticks in hive, min ticks in hive)
        79 => list(r, |r| {
            typed_entity_data(r)?;
            all(&[varint, varint], r)
        }),
        // break_sound
        83 => sound_event(r),
        // compostable: ResolvableInt
        84 => resolvable_int(r),
        // cooking_fuel, brewing_fuel: ResolvableInt, ResolvableFloat
        85 | 86 => resolvable_int(r).and_then(|_| resolvable_float(r)),
        // mob_visibility: entity types, visibility
        87 => holder_set(r).and_then(|_| f32_(r)),
        // painting/variant: Holder<PaintingVariant>
        109 => holder(r, |r| {
            all(&[varint, varint, ident], r)?;
            optional(r, nbt)?;
            optional(r, nbt)
        }),
        // sign_text_front, sign_text_back
        118 | 119 => sign_text(r),
        _ => return None,
    })
}

type Step = fn(&mut Reader) -> Result<()>;

fn all(steps: &[Step], r: &mut Reader) -> Result<()> {
    steps.iter().try_for_each(|step| step(r))
}
fn varint(r: &mut Reader) -> Result<()> {
    r.varint().map(drop)
}
fn bool_(r: &mut Reader) -> Result<()> {
    r.bool().map(drop)
}
fn f32_(r: &mut Reader) -> Result<()> {
    r.f32().map(drop)
}
fn f64_(r: &mut Reader) -> Result<()> {
    r.f64().map(drop)
}
fn i32_(r: &mut Reader) -> Result<()> {
    r.i32().map(drop)
}
fn i64_(r: &mut Reader) -> Result<()> {
    r.i64().map(drop)
}
fn string(r: &mut Reader) -> Result<()> {
    r.string(32767).map(drop)
}
/// Identifier.STREAM_CODEC (and ResourceKey.streamCodec): a UTF-8 string.
fn ident(r: &mut Reader) -> Result<()> {
    string(r)
}
/// A network tag: COMPOUND_TAG, ComponentSerialization.STREAM_CODEC and every
/// fromCodecWithRegistries value.
fn nbt(r: &mut Reader) -> Result<()> {
    r.nbt().map(drop)
}
fn uuid(r: &mut Reader) -> Result<()> {
    r.uuid().map(drop)
}
/// ByteBufCodecs.optional: a present flag, then the value.
fn optional(r: &mut Reader, value: impl Fn(&mut Reader) -> Result<()>) -> Result<()> {
    if r.bool()? { value(r) } else { Ok(()) }
}
/// ByteBufCodecs.list / collection: a VarInt count, then the elements.
fn list(r: &mut Reader, element: impl Fn(&mut Reader) -> Result<()>) -> Result<()> {
    let n = r.count(r.remaining().max(1) * 8)?;
    (0..n).try_for_each(|_| element(r))
}
/// ByteBufCodecs.either: true for the left codec.
fn either(
    r: &mut Reader,
    left: impl Fn(&mut Reader) -> Result<()>,
    right: impl Fn(&mut Reader) -> Result<()>,
) -> Result<()> {
    if r.bool()? { left(r) } else { right(r) }
}
/// ByteBufCodecs.holder: 0 and the inline value, or a registry id + 1.
fn holder(r: &mut Reader, direct: impl Fn(&mut Reader) -> Result<()>) -> Result<()> {
    if r.varint()? == 0 { direct(r) } else { Ok(()) }
}
/// ByteBufCodecs.holderSet: 0 and a tag's identifier, or the count + 1 and that many ids.
fn holder_set(r: &mut Reader) -> Result<()> {
    let n = r.varint()?;
    if n == 0 {
        return ident(r);
    }
    let n = usize::try_from(n - 1).map_err(|_| Error::new(format!("bad holder set size {n}")))?;
    if n > r.remaining() {
        return Err(Error::new(format!(
            "holder set of {n} in {} bytes",
            r.remaining()
        )));
    }
    (0..n).try_for_each(|_| varint(r))
}
/// SoundEvent.STREAM_CODEC: a holder of (location, optional fixed range).
fn sound_event(r: &mut Reader) -> Result<()> {
    holder(r, |r| ident(r).and_then(|_| optional(r, f32_)))
}
/// Filterable.streamCodec: the raw value and an optional filtered one.
fn filterable(r: &mut Reader, value: Step) -> Result<()> {
    value(r)?;
    optional(r, value)
}
/// ItemStackTemplate.STREAM_CODEC: item, count and a component patch.
fn item_template(r: &mut Reader) -> Result<()> {
    use crate::buf::Codec;
    item::Template::read(r).map(drop)
}
/// TypedEntityData.streamCodec: the type's registry id and its compound.
fn typed_entity_data(r: &mut Reader) -> Result<()> {
    varint(r)?;
    nbt(r)
}
/// MobEffectInstance: effect, then Details (amplifier, duration, ambient, particles, icon and an
/// optional hidden Details, recursively).
fn mob_effect_instance(r: &mut Reader) -> Result<()> {
    varint(r)?;
    effect_details(r)
}
fn effect_details(r: &mut Reader) -> Result<()> {
    all(&[varint, varint, bool_, bool_, bool_], r)?;
    optional(r, effect_details)
}
/// ConsumeEffect.STREAM_CODEC, dispatched on minecraft:consume_effect_type.
fn consume_effect(r: &mut Reader) -> Result<()> {
    match r.varint()? {
        // apply_effects: effects, probability
        0 => list(r, mob_effect_instance).and_then(|_| f32_(r)),
        // remove_effects
        1 => holder_set(r),
        // clear_all_effects
        2 => Ok(()),
        // teleport_randomly: diameter, directional particles
        3 => all(&[f32_, bool_], r),
        // play_sound
        4 => sound_event(r),
        other => Err(Error::new(format!("unknown consume effect type {other}"))),
    }
}
/// ItemAttributeModifiers.Entry: attribute, modifier (id, amount, operation), slot group and
/// display (default, hidden, or a text override).
fn attribute_modifier_entry(r: &mut Reader) -> Result<()> {
    all(&[varint, ident, f64_, varint, varint], r)?;
    match r.varint()? {
        0 | 1 => Ok(()),
        2 => nbt(r),
        other => Err(Error::new(format!("unknown attribute display {other}"))),
    }
}
/// BlockPredicate: optional blocks, optional state properties, optional NBT and the component
/// matchers.
fn block_predicate(r: &mut Reader) -> Result<()> {
    optional(r, holder_set)?;
    optional(r, |r| {
        list(r, |r| {
            string(r)?;
            // ValueMatcher: exact value, or an optional min and max
            either(r, string, |r| {
                optional(r, string).and_then(|_| optional(r, string))
            })
        })
    })?;
    optional(r, nbt)?;
    // DataComponentMatchers: exact components, then partial predicates
    list(r, |r| {
        let kind = r.varint()?;
        item::Component::read(kind, r).map(drop)
    })?;
    list(r, |r| {
        // a predicate type or a component type ("any value"), then its value as a tag
        bool_(r)?;
        varint(r)?;
        nbt(r)
    })
}
/// Tool: rules (blocks, optional speed, optional correct-for-drops), default speed, damage per
/// block, destroys blocks in creative.
fn tool(r: &mut Reader) -> Result<()> {
    list(r, |r| {
        holder_set(r)?;
        optional(r, f32_)?;
        optional(r, bool_)
    })?;
    all(&[f32_, varint, bool_], r)
}
/// Equippable: slot, equip sound, optional asset and camera overlay, optional allowed entities,
/// five flags and the shearing sound.
fn equippable(r: &mut Reader) -> Result<()> {
    varint(r)?;
    sound_event(r)?;
    optional(r, ident)?;
    optional(r, ident)?;
    optional(r, holder_set)?;
    all(&[bool_, bool_, bool_, bool_, bool_], r)?;
    sound_event(r)
}
/// BlocksAttacks: delay, cooldown scale, damage reductions, item damage, bypassed by, sounds.
fn blocks_attacks(r: &mut Reader) -> Result<()> {
    all(&[f32_, f32_], r)?;
    list(r, |r| {
        f32_(r)?;
        optional(r, holder_set)?;
        all(&[f32_, f32_], r)
    })?;
    all(&[f32_, f32_, f32_], r)?;
    optional(r, holder_set)?;
    optional(r, sound_event)?;
    optional(r, sound_event)
}
/// KineticWeapon: cooldown, delay, three optional conditions, movement, multiplier, two sounds.
fn kinetic_weapon(r: &mut Reader) -> Result<()> {
    all(&[varint, varint], r)?;
    for _ in 0..3 {
        optional(r, |r| all(&[varint, f32_, f32_], r))?;
    }
    all(&[f32_, f32_], r)?;
    optional(r, sound_event)?;
    optional(r, sound_event)
}
/// PotionContents: optional potion, optional colour, custom effects, optional name.
fn potion_contents(r: &mut Reader) -> Result<()> {
    optional(r, varint)?;
    optional(r, i32_)?;
    list(r, mob_effect_instance)?;
    optional(r, string)
}
/// WrittenBookContent: title (Filterable, 32 chars), author, generation, pages (Filterable text),
/// resolved.
fn written_book_content(r: &mut Reader) -> Result<()> {
    filterable(r, string)?;
    string(r)?;
    varint(r)?;
    list(r, |r| filterable(r, nbt))?;
    bool_(r)
}
/// FireworkExplosion: shape, colours, fade colours, trail, twinkle.
fn firework_explosion(r: &mut Reader) -> Result<()> {
    varint(r)?;
    list(r, i32_)?;
    list(r, i32_)?;
    all(&[bool_, bool_], r)
}
/// ResolvableProfile: a full GameProfile or a partial one, then the skin patch.
fn resolvable_profile(r: &mut Reader) -> Result<()> {
    either(
        r,
        |r| {
            uuid(r)?;
            string(r)?;
            profile_properties(r)
        },
        |r| {
            optional(r, string)?;
            optional(r, uuid)?;
            profile_properties(r)
        },
    )?;
    // PlayerSkin.Patch: optional body, cape and elytra textures, optional slim model
    optional(r, ident)?;
    optional(r, ident)?;
    optional(r, ident)?;
    optional(r, bool_)
}
/// GAME_PROFILE_PROPERTIES: (name, value, nullable signature).
fn profile_properties(r: &mut Reader) -> Result<()> {
    list(r, |r| {
        all(&[string, string], r)?;
        optional(r, string)
    })
}
/// ResolvableInt / ResolvableFloat: a constant or a context provider's key.
fn resolvable_int(r: &mut Reader) -> Result<()> {
    either(r, i32_, ident)
}
fn resolvable_float(r: &mut Reader) -> Result<()> {
    either(r, f32_, ident)
}
/// SignText: four lines, four optional filtered lines, colour, glowing.
fn sign_text(r: &mut Reader) -> Result<()> {
    (0..4).try_for_each(|_| nbt(r))?;
    optional(r, |r| (0..4).try_for_each(|_| nbt(r)))?;
    all(&[varint, bool_], r)
}
