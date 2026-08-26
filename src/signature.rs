//! OSTW source bindings to the canonical Workshop catalog (#118).
//!
//! This module owns ONLY genuinely OSTW-specific source binding/alias
//! metadata: the OSTW source name -> canonical catalog identity mapping for
//! the exercised builtin surface, and the OSTW source member name -> canonical
//! catalog member id mapping per exercised enum domain. All canonical
//! Workshop parameter/spelling and enum domain/member data lives in the
//! canonical catalog (`workshop-rs`); the
//! semantic phase
//! resolves builtins and enum domains through that catalog at the consume
//! sites. No OSTW `Elements.json` or upstream compiler table is copied;
//! every binding is exercised by the protect-ban reachable closure or a
//! committed pinned-reference probe under `compatibility/ostw/probes/`.

use workshop_rs::catalog::Kind;

/// One exercised builtin binding: OSTW source name -> (kind, canonical catalog id).
pub const BUILTIN_BINDINGS: &[(&str, (Kind, &str))] = &[
    (
        "WorkshopSettingInteger",
        (Kind::Value, "workshopSettingInteger"),
    ),
    (
        "WorkshopSettingToggle",
        (Kind::Value, "workshopSettingToggle"),
    ),
    (
        "WorkshopSettingCombo",
        (Kind::Value, "workshopSettingCombo"),
    ),
    ("AllPlayers", (Kind::Value, "allPlayers")),
    ("AllHeroes", (Kind::Value, "allHeroes")),
    ("AllTankHeroes", (Kind::Value, "allTankHeroes")),
    ("AllDamageHeroes", (Kind::Value, "allDamageHeroes")),
    ("AllSupportHeroes", (Kind::Value, "allSupportHeroes")),
    ("AllowedHeroes", (Kind::Value, "allowedHeroes")),
    ("EventPlayer", (Kind::Value, "eventPlayer")),
    ("LocalPlayer", (Kind::Value, "localPlayer")),
    ("TeamOf", (Kind::Value, "teamOf")),
    ("OppositeTeamOf", (Kind::Value, "oppositeTeamOf")),
    ("NumberOfPlayers", (Kind::Value, "numberOfPlayers")),
    ("ArrayContains", (Kind::Value, "arrayContains")),
    ("ArrayElement", (Kind::Value, "currentArrayElement")),
    ("CurrentArrayIndex", (Kind::Value, "currentArrayIndex")),
    ("CountOf", (Kind::Value, "countOf")),
    ("IndexOfArrayValue", (Kind::Value, "indexOfArrayValue")),
    ("RandomValueInArray", (Kind::Value, "randomValueInArray")),
    ("MappedArray", (Kind::Value, "mappedArray")),
    ("FilteredArray", (Kind::Value, "filteredArray")),
    ("SortedArray", (Kind::Value, "sortedArray")),
    ("LastOf", (Kind::Value, "lastOf")),
    ("EmptyArray", (Kind::Value, "emptyArray")),
    ("RemoveFromArray", (Kind::Value, "removeFromArray")),
    ("Append", (Kind::Value, "appendToArray")),
    ("Max", (Kind::Value, "max")),
    ("Min", (Kind::Value, "min")),
    ("RoundToInteger", (Kind::Value, "roundToInteger")),
    ("Vector", (Kind::Value, "vector")),
    ("CrossProduct", (Kind::Value, "crossProduct")),
    ("DirectionFromAngles", (Kind::Value, "directionFromAngles")),
    (
        "HorizontalAngleFromDirection",
        (Kind::Value, "horizontalAngleFromDirection"),
    ),
    (
        "VerticalAngleFromDirection",
        (Kind::Value, "verticalAngleFromDirection"),
    ),
    ("Forward", (Kind::Value, "forward")),
    ("CustomColor", (Kind::Value, "customColor")),
    ("HasSpawned", (Kind::Value, "hasSpawned")),
    ("IsButtonHeld", (Kind::Value, "isButtonHeld")),
    ("IsInSpawnRoom", (Kind::Value, "isInSpawnRoom")),
    ("IsTrueForAll", (Kind::Value, "isTrueForAll")),
    ("IsWaitingForPlayers", (Kind::Value, "isWaitingForPlayers")),
    ("CurrentMap", (Kind::Value, "currentMap")),
    ("EvaluateOnce", (Kind::Value, "evaluateOnce")),
    ("UpdateEveryFrame", (Kind::Value, "updateEveryFrame")),
    ("LastCreatedEntity", (Kind::Value, "lastCreatedEntity")),
    ("LastTextID", (Kind::Value, "lastTextId")),
    ("HeroIconString", (Kind::Value, "heroIconString")),
    ("AbilityIconString", (Kind::Value, "abilityIconString")),
    ("IconString", (Kind::Value, "iconString")),
    ("InputBindingString", (Kind::Value, "inputBindingString")),
    ("BigMessage", (Kind::Action, "bigMessage")),
    ("SmallMessage", (Kind::Action, "smallMessage")),
    ("Wait", (Kind::Action, "wait")),
    ("WaitUntil", (Kind::Action, "waitUntil")),
    ("MinWait", (Kind::Action, "wait")),
    ("Skip", (Kind::Action, "skip")),
    (
        "LoopIfConditionIsTrue",
        (Kind::Action, "loopIfConditionIsTrue"),
    ),
    ("AbortIf", (Kind::Action, "abortIf")),
    ("ModifyVariable", (Kind::Action, "modifyGlobalVariable")),
    ("CreateEffect", (Kind::Action, "createEffect")),
    ("CreateInWorldText", (Kind::Action, "createInWorldText")),
    (
        "CreateProgressBarInWorldText",
        (Kind::Action, "createProgressBarInWorldText"),
    ),
    ("CreateHudText", (Kind::Action, "createHudText")),
    ("PlayEffect", (Kind::Action, "playEffect")),
    ("StartCamera", (Kind::Action, "startCamera")),
    ("StopCamera", (Kind::Action, "stopCamera")),
    ("StartGameMode", (Kind::Action, "startGameMode")),
    ("SetInvisible", (Kind::Action, "setInvisibility")),
    ("SetGravity", (Kind::Action, "setGravity")),
    ("SetAllowedHeroes", (Kind::Action, "setAllowedHeroes")),
    ("ForcePlayerHero", (Kind::Action, "forcePlayerHero")),
    ("StopForcingHero", (Kind::Action, "stopForcingHero")),
    ("ForceThrottle", (Kind::Action, "forceThrottle")),
    ("StopForcingThrottle", (Kind::Action, "stopForcingThrottle")),
    ("DisableGameModeHud", (Kind::Action, "disableGameModeHud")),
    (
        "DisableGameModeInworldUI",
        (Kind::Action, "disableGameModeInworldUI"),
    ),
    ("DisableHeroHud", (Kind::Action, "disableHeroHud")),
    ("DisableScoreboard", (Kind::Action, "disableScoreboard")),
    (
        "DisableInspectorRecording",
        (Kind::Action, "disableInspector"),
    ),
    ("EnableGameModeHud", (Kind::Action, "enableGameModeHud")),
    (
        "EnableGameModeInworldUI",
        (Kind::Action, "enableGameModeInworldUI"),
    ),
    ("EnableHeroHud", (Kind::Action, "enableHeroHud")),
    ("EnableScoreboard", (Kind::Action, "enableScoreboard")),
    (
        "EnableInspectorRecording",
        (Kind::Action, "enableInspectorRecording"),
    ),
    (
        "DisableMovementCollisionWithEnvironment",
        (Kind::Action, "disableMovementCollisionWithEnvironment"),
    ),
    (
        "DisableMovementCollisionWithPlayers",
        (Kind::Action, "disableMovementCollisionWithPlayers"),
    ),
    (
        "EnableMovementCollisionWithEnvironment",
        (Kind::Action, "enableMovementCollisionWithEnvironment"),
    ),
    (
        "EnableMovementCollisionWithPlayers",
        (Kind::Action, "enableMovementCollisionWithPlayers"),
    ),
    ("DisallowButton", (Kind::Action, "disallowButton")),
    ("AllowButton", (Kind::Action, "allowButton")),
    ("DestroyHudText", (Kind::Action, "destroyHudText")),
    ("DestroyInWorldText", (Kind::Action, "destroyInWorldText")),
    ("DestroyEffect", (Kind::Action, "destroyEffect")),
    (
        "DestroyAllProgressBarHudText",
        (Kind::Action, "destroyAllProgressBarHudText"),
    ),
    (
        "DestroyAllProgressBarInWorldText",
        (Kind::Action, "destroyAllProgressBarInWorldText"),
    ),
    ("StopChasingVariable", (Kind::Action, "stopChasingVariable")),
    // The generic OSTW chase spelling is lowered to the canonical chase
    // family; workshop-rs emits the global/player spelling from the variable
    // value shape.
    ("ChaseVariableAtRate", (Kind::Action, "chaseAtRate")),
    ("Teleport", (Kind::Action, "teleport")),
];

/// Resolve an exercised Workshop builtin by its OSTW source name.
pub fn builtin(name: &str) -> Option<(Kind, &'static str)> {
    BUILTIN_BINDINGS
        .iter()
        .find(|(source, _)| *source == name)
        .map(|(_, binding)| *binding)
}

/// One exercised enum domain binding: the canonical catalog domain plus the
/// OSTW source member name -> canonical catalog member id mapping.
pub struct EnumDomainBinding {
    /// The canonical catalog domain name.
    pub domain: &'static str,
    /// OSTW source member name -> canonical catalog member id.
    pub members: &'static [(&'static str, &'static str)],
}

pub const ENUM_DOMAIN_BINDINGS: &[(&str, EnumDomainBinding)] = &[
    (
        "Team",
        EnumDomainBinding {
            domain: "Team",
            members: &[("All", "ALL"), ("Team1", "TEAM_1"), ("Team2", "TEAM_2")],
        },
    ),
    (
        "Button",
        EnumDomainBinding {
            domain: "Button",
            members: &[
                ("PrimaryFire", "PRIMARY_FIRE"),
                ("SecondaryFire", "SECONDARY_FIRE"),
                ("Ability1", "ABILITY_1"),
                ("Ability2", "ABILITY_2"),
                ("Ultimate", "ULTIMATE"),
                ("Crouch", "CROUCH"),
                ("Interact", "INTERACT"),
                ("Jump", "JUMP"),
                ("Melee", "MELEE"),
                ("Reload", "RELOAD"),
            ],
        },
    ),
    (
        "Clipping",
        EnumDomainBinding {
            domain: "Clipping",
            members: &[
                ("DoNotClip", "DO_NOT_CLIP"),
                ("ClipAgainstSurfaces", "CLIP_AGAINST_SURFACES"),
            ],
        },
    ),
    (
        "Color",
        EnumDomainBinding {
            domain: "Color",
            members: &[
                ("White", "WHITE"),
                ("Yellow", "YELLOW"),
                ("Green", "GREEN"),
                ("Purple", "PURPLE"),
                ("Red", "RED"),
                ("Blue", "BLUE"),
                ("Aqua", "AQUA"),
                ("Orange", "ORANGE"),
                ("SkyBlue", "SKY_BLUE"),
                ("Turquoise", "TURQUOISE"),
                ("LimeGreen", "LIME_GREEN"),
                ("Gray", "GRAY"),
                ("Violet", "VIOLET"),
                ("Rose", "ROSE"),
                ("Black", "BLACK"),
                ("Team1", "TEAM_1"),
                ("Team2", "TEAM_2"),
            ],
        },
    ),
    (
        "Effect",
        EnumDomainBinding {
            domain: "Effect",
            members: &[("Orb", "ORB")],
        },
    ),
    (
        "EffectRev",
        EnumDomainBinding {
            domain: "EffectReeval",
            members: &[(
                "VisibleToPositionAndRadius",
                "VISIBLE_TO_POSITION_AND_RADIUS",
            )],
        },
    ),
    (
        "Hero",
        EnumDomainBinding {
            domain: "Hero",
            members: &[
                ("Dva", "DVA"),
                ("Orisa", "ORISA"),
                ("Reinhardt", "REINHARDT"),
                ("Roadhog", "ROADHOG"),
                ("Sigma", "SIGMA"),
                ("WreckingBall", "WRECKING_BALL"),
                ("Winston", "WINSTON"),
                ("Zarya", "ZARYA"),
                ("Ashe", "ASHE"),
                ("Bastion", "BASTION"),
                ("Cassidy", "CASSIDY"),
                ("Doomfist", "DOOMFIST"),
                ("Echo", "ECHO"),
                ("Genji", "GENJI"),
                ("Hanzo", "HANZO"),
                ("Junkrat", "JUNKRAT"),
                ("Mei", "MEI"),
                ("Pharah", "PHARAH"),
                ("Reaper", "REAPER"),
                ("Soldier76", "SOLDIER_76"),
                ("Symmetra", "SYMMETRA"),
                ("Sombra", "SOMBRA"),
                ("Tracer", "TRACER"),
                ("Torbjorn", "TORBJORN"),
                ("Widowmaker", "WIDOWMAKER"),
                ("Ana", "ANA"),
                ("Brigitte", "BRIGITTE"),
                ("Baptiste", "BAPTISTE"),
                ("Lucio", "LUCIO"),
                ("Moira", "MOIRA"),
                ("Mercy", "MERCY"),
                ("Zenyatta", "ZENYATTA"),
            ],
        },
    ),
    (
        "HudTextRev",
        EnumDomainBinding {
            domain: "HudReeval",
            members: &[
                ("VisibleTo", "VISIBILITY"),
                ("VisibleToAndString", "VISIBILITY_AND_STRING"),
                ("VisibleToStringAndColor", "VISIBLE_TO_STRING_AND_COLOR"),
                ("VisibleToAndColor", "VISIBLE_TO_AND_COLOR"),
            ],
        },
    ),
    (
        "Icon",
        EnumDomainBinding {
            domain: "Icon",
            members: &[
                ("No", "NO"),
                ("QuestionMark", "QUESTION_MARK"),
                ("Skull", "SKULL"),
                ("Checkmark", "CHECKMARK"),
                ("RingThin", "RING_THIN"),
            ],
        },
    ),
    (
        "InvisibleTo",
        EnumDomainBinding {
            domain: "Invis",
            members: &[("All", "ALL"), ("None", "NONE")],
        },
    ),
    (
        "Map",
        EnumDomainBinding {
            domain: "Map",
            members: &[
                ("Hanamura", "HANAMURA"),
                ("Hanamura_Winter", "HANAMURA_WINTER"),
                ("Horizon_Lunar_Colony", "HORIZON_LUNAR_COLONY"),
                ("Paris", "PARIS"),
                ("Temple_of_Anubis", "TEMPLE_OF_ANUBIS"),
                ("Volskaya_Industries", "VOLSKAYA_INDUSTRIES"),
                ("Hanaoka", "HANAOKA"),
                ("Throne_of_Anubis", "THRONE_OF_ANUBIS"),
                ("Antarctic_Peninsula", "ANTARCTIC_PENINSULA"),
                ("Busan", "BUSAN"),
                ("Ilios", "ILIOS"),
                ("Lijiang_Tower", "LIJIANG_TOWER"),
                ("Lijiang_Tower_Lunar", "LIJIANG_TOWER_LUNAR"),
                ("Nepal", "NEPAL"),
                ("Oasis", "OASIS"),
                ("Samoa", "SAMOA"),
                ("Circuit_Royal", "CIRCUIT_ROYAL"),
                ("Dorado", "DORADO"),
                ("Havana", "HAVANA"),
                ("Junkertown", "JUNKERTOWN"),
                ("Rialto", "RIALTO"),
                ("Route_66", "ROUTE_66"),
                ("Shambali_Monastery", "SHAMBALI_MONASTERY"),
                ("Watchpoint_Gibraltar", "WATCHPOINT_GIBRALTAR"),
                ("Aatlis", "AATLIS"),
                ("New_Junk_City", "NEW_JUNK_CITY"),
                ("Suravasa", "SURAVASA"),
                ("Blizzard_World", "BLIZZARD_WORLD"),
                ("Blizzard_World_Winter", "BLIZZARD_WORLD_WINTER"),
                ("Eichenwalde", "EICHENWALDE"),
                ("Eichenwalde_Halloween", "EICHENWALDE_HALLOWEEN"),
                ("Hollywood", "HOLLYWOOD"),
                ("Hollywood_Halloween", "HOLLYWOOD_HALLOWEEN"),
                ("Kings_Row", "KINGS_ROW"),
                ("Kings_Row_Winter", "KINGS_ROW_WINTER"),
                ("Midtown", "MIDTOWN"),
                ("Numbani", "NUMBANI"),
                ("Paraiso", "PARAISO"),
                ("Colosseo", "COLOSSEO"),
                ("Esperanca", "ESPERANCA"),
                ("New_Queen_Street", "NEW_QUEEN_STREET"),
                ("Runasapi", "RUNASAPI"),
            ],
        },
    ),
    (
        "InworldTextRev",
        EnumDomainBinding {
            domain: "InworldTextReeval",
            members: &[
                ("VisibleTo", "VISIBLE_TO"),
                ("VisibleToAndColor", "VISIBLE_TO_AND_COLOR"),
                ("VisibleToAndPosition", "VISIBLE_TO_AND_POSITION"),
                ("VisibleToAndString", "VISIBLE_TO_AND_STRING"),
                ("VisibleToPositionAndColor", "VISIBLE_TO_POSITION_AND_COLOR"),
                (
                    "VisibleToPositionAndString",
                    "VISIBLE_TO_POSITION_AND_STRING",
                ),
                (
                    "VisibleToPositionStringAndColor",
                    "VISIBLE_TO_POSITION_STRING_AND_COLOR",
                ),
                ("VisibleToStringAndColor", "VISIBLE_TO_STRING_AND_COLOR"),
                ("String", "STRING"),
            ],
        },
    ),
    (
        "WorldTextRev",
        EnumDomainBinding {
            domain: "WorldTextReeval",
            members: &[
                ("Color", "COLOR"),
                ("None", "NONE"),
                ("String", "STRING"),
                ("StringAndColor", "STRING_AND_COLOR"),
                ("VisibleTo", "VISIBILITY"),
                ("VisibleToAndColor", "VISIBILITY_AND_COLOR"),
                ("VisibleToAndPosition", "VISIBILITY_AND_POSITION"),
                ("VisibleToAndString", "VISIBILITY_AND_STRING"),
                ("VisibleToPositionAndColor", "VISIBILITY_POSITION_AND_COLOR"),
                (
                    "VisibleToPositionAndString",
                    "VISIBILITY_POSITION_AND_STRING",
                ),
                (
                    "VisibleToPositionStringAndColor",
                    "VISIBILITY_POSITION_STRING_AND_COLOR",
                ),
                ("VisibleToStringAndColor", "VISIBILITY_STRING_AND_COLOR"),
            ],
        },
    ),
    (
        "Location",
        EnumDomainBinding {
            domain: "HudPosition",
            members: &[("Left", "LEFT"), ("Right", "RIGHT")],
        },
    ),
    (
        "Operation",
        EnumDomainBinding {
            domain: "Operation",
            members: &[
                ("AppendToArray", "APPEND_TO_ARRAY"),
                ("RemoveFromArrayByValue", "REMOVE_FROM_ARRAY_BY_VALUE"),
                ("RemoveFromArrayByIndex", "REMOVE_FROM_ARRAY_BY_INDEX"),
            ],
        },
    ),
    (
        "PlayEffect",
        EnumDomainBinding {
            domain: "DynamicEffect",
            members: &[
                ("BuffImpactSound", "BUFF_IMPACT_SOUND"),
                ("DebuffImpactSound", "DEBUFF_IMPACT_SOUND"),
                ("BuffExplosionSound", "BUFF_EXPLOSION_SOUND"),
                ("ExplosionSound", "EXPLOSION_SOUND"),
                ("RingExplosionSound", "RING_EXPLOSION"),
            ],
        },
    ),
    (
        "ProgressBarWorldEvaluation",
        EnumDomainBinding {
            domain: "ProgressBarWorldReeval",
            members: &[("VisibleToAndValues", "VISIBLE_TO_AND_VALUES")],
        },
    ),
    (
        "RateChaseReevaluation",
        EnumDomainBinding {
            domain: "ChaseRateReeval",
            members: &[
                ("None", "NONE"),
                ("DestinationAndRate", "DESTINATION_AND_RATE"),
            ],
        },
    ),
    (
        "Rounding",
        EnumDomainBinding {
            domain: "Rounding",
            members: &[("Up", "UP"), ("Down", "DOWN"), ("Nearest", "NEAREST")],
        },
    ),
    (
        "Spectators",
        EnumDomainBinding {
            domain: "SpecVisibility",
            members: &[
                ("DefaultVisibility", "DEFAULT"),
                ("VisibleAlways", "VISIBLE_ALWAYS"),
                ("VisibleNever", "VISIBLE_NEVER"),
            ],
        },
    ),
    (
        "WaitBehavior",
        EnumDomainBinding {
            domain: "Wait",
            members: &[
                ("AbortWhenFalse", "ABORT_WHEN_FALSE"),
                ("IgnoreCondition", "IGNORE_CONDITION"),
            ],
        },
    ),
    (
        "Vector",
        EnumDomainBinding {
            domain: "Vector",
            members: &[("Forward", "FORWARD"), ("Backward", "BACKWARD")],
        },
    ),
];

/// Resolve an exercised builtin enum domain by its OSTW source name.
pub fn enum_domain(name: &str) -> Option<&'static EnumDomainBinding> {
    ENUM_DOMAIN_BINDINGS
        .iter()
        .find(|(source, _)| *source == name)
        .map(|(_, binding)| binding)
}
