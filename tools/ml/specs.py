"""Per-model conversion specs for the Maverick model zoo.

Each entry names the tensor-in / tensor-out neural core inside one of Maverick's
TorchScript archives, the fixed input shapes Maverick contracts for that core, and
the output names the Rust side reads back. Everything outside the core — validation,
resampling, filtering, windowing, post-processing — is deliberately excluded: it
is deterministic and belongs in shared Rust, per Maverick's docs/ml.md.

Every shape here was established empirically: the core was called until its own
weights accepted the tensor, and the accepted shape recorded. `convert.py`
re-executes each core at these shapes on every run, so a wrong entry fails the
pipeline rather than silently shipping.

Fields
    source      decrypted TorchScript archive under ../decrypted_models
    core        dotted attribute path to the neural core inside that archive
    inputs      ordered [(name, shape, dtype)]
    const_args  trailing non-tensor arguments baked into the trace
    outputs     names for the flattened output tensors, in order
    algorithm   Maverick algorithm id, matching mav-analytic's model registry
    version     algorithm version, tied to the archive it was trained into
"""

F32 = "float32"
I64 = "int64"


def spec(source, core, inputs, outputs, const_args=(), algorithm=None, version=None, role="", notes="", core_method=None, arg_template=None, rebuild=None, rebuild_config=None, int_bounds=None):
    return dict(
        source=source,
        core=core,
        core_method=core_method,
        arg_template=arg_template,
        rebuild=rebuild,
        rebuild_config=rebuild_config,
        inputs=inputs,
        outputs=outputs,
        const_args=list(const_args),
        int_bounds=dict(int_bounds or {}),
        algorithm=algorithm,
        version=version,
        role=role,
        notes=notes,
    )


SPECS = {
    # ---- PPG front-end -------------------------------------------------------
    "pulsenet_foundation": spec(
        source="halite_1_2_0.pt",
        core="pulsenet_model.trained_model",
        inputs=[("ppg", (1, 1, 1500), F32)],
        outputs=["embeddings"],
        const_args=[True],
        algorithm="pulsenet_foundation_embeddings",
        version="0.4.0",
        role="PPG foundation encoder: 30 s of detrended PPG at 50 Hz to a 256-d embedding",
        notes=(
            "EfficientNet1D encoder, PulseNet-Foundation v0.4.0, from the halite 1.2.0 archive. "
            "The wrapper's own moving-average detrend chain is ported to Rust; this core takes "
            "the already-filtered segment."
        ),
    ),
    "halite_ppg_score": spec(
        source="halite_1_2_0.pt",
        core="embedding_model",
        inputs=[("embeddings", (1, 256), F32)],
        outputs=["ppg_score"],
        algorithm="halite_ppg_score",
        version="1.2.0",
        role="Scores one PulseNet embedding into the hypertension-risk PPG score",
    ),
    "halite_risk_tree": spec(
        source="halite_1_2_0.pt",
        core="tree_model",
        inputs=[("features", (1, 13), F32)],
        outputs=["label", "probabilities"],
        algorithm="halite_hypertension_risk",
        version="1.2.0",
        role="Gradient-boosted head over user info, baselines and the aggregated PPG score",
        notes="Feature vector is user_info(4) || baselines(8) || aggregated ppg score(1).",
    ),
    # ---- Cardiovascular ------------------------------------------------------
    "cva_predictor": spec(
        source="cva_2_1_0.pt",
        core="cva_pd",
        inputs=[
            ("pulses", (1, 1499), F32),
            ("features", (1, 5), F32),
            ("demographics", (1, 5), F32),
        ],
        outputs=[
            "cva",
            "pwv",
            "cva_uncalibrated",
            "prediction_quality",
            "signal_quality",
            "sbp",
            "dbp",
            "embeddings",
        ],
        algorithm="cva_predictor",
        version="2.1.0",
        role="PPG transformer: cardiovascular age, pulse-wave velocity and blood-pressure probes",
        notes=(
            "features are the preprocessor's (mean_dc, max_min, accepted, snr, hr); demographics "
            "are (sex, ring size class, age, ring width class, bmi)."
        ),
    ),
    "cva_predictor_v1": spec(
        source="cva_1_3_0.pt",
        core="cva_pd",
        inputs=[
            ("pulses", (1, 1473), F32),
            ("vpgs", (1, 1473), F32),
            ("apgs", (1, 1473), F32),
            ("signal_quality_features", (10, 5), F32),
            ("demographics", (10, 5), F32),
        ],
        outputs=["cva", "pwv", "prediction_quality", "signal_quality"],
        algorithm="cva_predictor",
        version="1.3.0",
        role="Previous-generation CNN cardiovascular-age predictor over pulse/VPG/APG triples",
        notes="The 1,473-sample pulse train is consumed as ten 136-sample windows.",
    ),
    # ---- Sleep ---------------------------------------------------------------
    "sleepnet_moonstone": spec(
        source="sleepnet_moonstone_1_2_0.pt",
        core="_model_runner.trained_model",
        inputs=[
            ("high_res", (1, 115_200, 3), F32),
            ("low_res", (1, 1_800, 1), F32),
        ],
        outputs=["staging_logits", "apnea_logits"],
        algorithm="sleepnet_moonstone",
        version="1.2.0",
        role="Sleep stages, apnea and SpO2 events over a 15-hour night",
        notes=(
            "1,800 thirty-second epochs. high_res carries 64 samples per epoch across three "
            "channels; low_res carries one value per epoch."
        ),
    ),
    "sleepnet_bdi": spec(
        source="sleepnet_bdi_0_4_0.pt",
        core="_model_runner.trained_model",
        inputs=[("high_res", (1, 115_200, 2), F32)],
        outputs=["staging_logits", "apnea_logits"],
        algorithm="sleepnet_bdi",
        version="0.4.0",
        role="Sleep stages and apnea events from interbeat intervals alone",
    ),
    "sleepnet_bdi_v3": spec(
        source="sleepnet_bdi_0_3_0.pt",
        core="_model_runner.trained_model",
        inputs=[("high_res", (1, 115_200, 2), F32)],
        outputs=["staging_logits", "apnea_logits"],
        algorithm="sleepnet_bdi",
        version="0.3.0",
        role="Previous-generation interbeat-interval sleep-staging network",
    ),
    # ---- Heart rate ----------------------------------------------------------
    "whr_unet": spec(
        source="whr_2_7_1.pt",
        core="predictor.unet_model",
        inputs=[
            ("images", (1, 2, 128, 128), F32),
            ("vectors", (1, 1, 128), F32),
            ("scalars", (1, 9), F32),
        ],
        outputs=["heart_rate", "segmentation"],
        algorithm="workout_heart_rate",
        version="2.7.1",
        role="U-Net workout heart-rate regressor over a 128-bin PPG spectrogram window",
    ),
    "awhr_imputation": spec(
        source="awhr_imputation_1_2_0.pt",
        core="impute_net",
        inputs=[("window", (1, 60, 13), F32)],
        outputs=["imputed_hr"],
        algorithm="awake_heart_rate_imputation",
        version="1.2.0",
        role="Bidirectional LSTM imputing awake heart rate over a 60-step context window",
    ),
    "awhr_profile_selector": spec(
        source="awhr_profile_selector_1_0_1.pt",
        core="model",
        inputs=[("features", (1, 60, 19), F32)],
        outputs=["profile_logits"],
        algorithm="awake_heart_rate_profile",
        version="1.0.1",
        role="Activity-profile classifier selecting the awake-HR imputation profile",
    ),
    "dhrv_imputation": spec(
        source="dhrv_imputation_1_1_0.pt",
        core="rf_net",
        inputs=[("features", (1, 10), F32)],
        outputs=["imputed_dhrv"],
        algorithm="daytime_hrv_imputation",
        version="1.1.0",
        role="Daytime HRV imputation from temperature, MET, HR and their baselines",
    ),
    # ---- Activity and movement ----------------------------------------------
    "activity_detection": spec(
        source="automatic_activity_detection_3_0_8.pt",
        core="predictor",
        inputs=[("features", (1, 64, 77), F32)],
        outputs=["activity_output"],
        algorithm="automatic_activity_detection",
        version="3.0.8",
        role="Activity classification over 64 candidate segments of 77 features",
    ),
    "activity_transition": spec(
        source="automatic_activity_detection_3_1_11.pt",
        core="segmentation.activity_transition_predictor",
        inputs=[("features", (1, 64, 29), F32)],
        outputs=["transition_logits"],
        algorithm="activity_transition_segmentation",
        version="3.1.11",
        role="Activity-transition segmentation over a 64-minute window",
    ),
    "step_counter": spec(
        source="step_counter_1_3_0.pt",
        core="model",
        inputs=[("motion", (1, 19), F32)],
        outputs=["steps"],
        algorithm="step_counter",
        version="1.3.0",
        role="Step count with eligibility gating from one step-motion feature vector",
    ),
    "energy_expenditure_hr": spec(
        source="energy_expenditure_1_0_0.pt",
        core="energy_expenditure_model_hr",
        inputs=[("features", (1, 50), F32)],
        outputs=["energy"],
        algorithm="energy_expenditure_hr",
        version="1.0.0",
        role="Active energy expenditure for a window where heart rate is available",
    ),
    # ---- Daily health --------------------------------------------------------
    "illness_detection": spec(
        source="illness_detection_0_5_1.pt",
        core="_model_runner.trained_model",
        inputs=[
            ("scalars", (1, 4), F32),
            ("time_series", (1, 8, 30), F32),
        ],
        outputs=["illness_probability"],
        algorithm="illness_detection",
        version="0.5.1",
        role="Illness likelihood from thirty days of eight daily biometric deviations",
    ),
    # ---- Split cores -------------------------------------------------------
    # Each of these is the neural half of a core whose wrapper branches on its data.
    # The branch moves to Rust; the tensor arithmetic converts. See docs/ml.md.

    "activity_segments": spec(
        source="automatic_activity_detection_3_1_11.pt",
        core="predictor",
        inputs=[("segments", (1, 16, 128), F32)],
        outputs=["primary", "attention", "secondary", "combined"],
        algorithm="activity_segments",
        version="3.1.11",
        role="Segment-level activity prediction over a 16-segment window",
        notes=(
            "The 3.1.11 archive's main head, 3.45 M of its 3.56 M parameters. The "
            "activity_transition core that shipped first is its 87 k segmentation companion."
        ),
    ),
    "behavior_embedding": spec(
        source="automatic_activity_detection_3_1_11.pt",
        core="behavior_embedding",
        inputs=[("behavior_ids", (1, 16), I64)],
        int_bounds={"behavior_ids": 89},
        outputs=["embeddings"],
        # The custom-id path routes through `searchsorted`, which neither converter lowers.
        # `False` selects the plain table lookup, and remapping a custom id to a table index
        # is a dictionary lookup that belongs in Rust anyway.
        const_args=[False],
        algorithm="behavior_embedding",
        version="3.1.11",
        role="Embeds sixteen behaviour ids into the activity model's 256-d space",
    ),
    "source_embedding": spec(
        source="automatic_activity_detection_3_1_11.pt",
        core="history_preprocessor.source_extractor.source_embedding",
        inputs=[("source_ids", (1, 16), I64)],
        int_bounds={"source_ids": 11},
        outputs=["source_features"],
        algorithm="activity_source_embedding",
        version="3.1.11",
        role="Embeds each segment's provenance — how the activity was recorded — into eight values",
        notes=(
            "An eleven-row table, the archive's last 88 parameters. Its wrapper clamps an "
            "out-of-range id to row zero and then writes three of the eight values back over "
            "the is_labeling, is_workout and is_sleep feature slots; both are Rust's."
        ),
    ),
    "energy_expenditure_no_hr": spec(
        source="energy_expenditure_1_0_0.pt",
        core="energy_expenditure_model_no_hr",
        inputs=[("features", (1, 42), F32)],
        outputs=["energy"],
        algorithm="energy_expenditure_no_hr",
        version="1.0.0",
        role="Active energy expenditure for a window with no usable heart rate",
        notes="The sibling branch of energy_expenditure_hr; Rust picks between them.",
    ),
    "step_eligibility": spec(
        source="step_counter_1_3_0.pt",
        core="model.step_eligibility_model.features_extractor",
        inputs=[("motion", (1, 19), F32)],
        outputs=["eligibility_features"],
        algorithm="step_eligibility",
        version="1.3.0",
        role="Step-eligibility features from one step-motion vector",
        notes=(
            "7,804 of the step counter's 7,811 parameters. The boolean-mask gating that "
            "LiteRT rejects sits in the parent module and belongs in Rust."
        ),
    ),
    "awhr_profile_core": spec(
        source="awhr_profile_selector_1_0_1.pt",
        core="model.activity_core_model",
        inputs=[("features", (60, 19), F32)],
        outputs=["profile_features"],
        algorithm="awake_heart_rate_profile",
        version="1.0.1",
        role="Per-step activity features behind the awake-HR profile choice",
        notes="Same split as step_eligibility: the mask lives in the parent, so it moves to Rust.",
    ),
    "whr_unet_encoder": spec(
        source="whr_2_7_1.pt",
        core="predictor.unet_model.unet",
        inputs=[("images", (1, 4, 128, 128), F32)],
        outputs=["features"],
        algorithm="workout_heart_rate_unet",
        version="2.7.1",
        role="U-Net over the workout PPG spectrogram window",
    ),
    "whr_unet_head": spec(
        source="whr_2_7_1.pt",
        core="predictor.unet_model.final",
        inputs=[
            ("vectors", (1, 1, 128), F32),
            ("scalars", (1, 7), F32),
            ("features", (1, 2, 128, 128), F32),
        ],
        outputs=["heart_rate"],
        algorithm="workout_heart_rate_head",
        version="2.7.1",
        role="Recurrent head turning U-Net features into a per-column heart rate",
        notes=(
            "Together with whr_unet_encoder this is the whole of workout heart rate. The "
            "aten.equal.default guard that blocked the combined core is in the glue between "
            "them, which is now Rust."
        ),
    ),
    "cva_encoder": spec(
        source="cva_2_1_0.pt",
        core="cva_pd.base_model",
        core_method="get_embeddings_1",
        inputs=[("pulses", (1, 1024), F32)],
        outputs=["embeddings"],
        algorithm="cva_encoder",
        version="2.1.0",
        role="PPG transformer encoder: 1,024 normalised pulse samples to a 128-d embedding",
        notes=(
            "`forward` on this module computes a contrastive training loss, not an inference "
            "result; `get_embeddings_1` is the entry its predictor calls. 1,024 is the model's "
            "own block_size, which is why the 1,499-sample pulse train is truncated to it."
        ),
    ),

    # The 3.1.11 predictor's four tensor heads. The parent module assembles them with a
    # list append inside a conditional, which coremltools cannot lower and which is
    # composition logic rather than arithmetic — so the heads convert and the assembly
    # is Rust's. Together they are 2.91 M of the parent's 3.45 M parameters.

    "activity_context_embedding": spec(
        source="automatic_activity_detection_3_1_11.pt",
        core="predictor.context_embedding",
        inputs=[("context", (1, 16, 85), F32)],
        outputs=["embeddings"],
        algorithm="activity_context_embedding",
        version="3.1.11",
        role="Embeds 85 context features per segment into a 312-d space",
    ),
    "activity_history_transformer": spec(
        source="automatic_activity_detection_3_1_11.pt",
        core="predictor.history_segments_transformer",
        inputs=[("segments", (1, 16, 573), F32)],
        outputs=["encoded", "attention"],
        algorithm="activity_history_transformer",
        version="3.1.11",
        role="Self-attention over sixteen history segments; also returns its attention map",
    ),
    "activity_primary_segments": spec(
        source="automatic_activity_detection_3_1_11.pt",
        core="predictor.primary_segments_predictor",
        inputs=[("features", (1, 16, 110), F32)],
        outputs=["segment_output"],
        algorithm="activity_primary_segments",
        version="3.1.11",
        role="Primary per-segment activity head",
    ),
    "activity_secondary_segments": spec(
        source="automatic_activity_detection_3_1_11.pt",
        core="predictor.secondary_segments_predictor",
        inputs=[("features", (1, 16, 336), F32)],
        outputs=["segment_output"],
        algorithm="activity_secondary_segments",
        version="3.1.11",
        role="Secondary per-segment activity head, over the history encoder's output",
    ),

    # Completing archives that already ship most of their parameters. Each of these is the
    # remainder of a core whose bulk converted, so the archive reaches full coverage.

    "step_head": spec(
        source="step_counter_1_3_0.pt",
        core="model.step_eligibility_model.head",
        inputs=[("eligibility_features", (1, 4), F32)],
        outputs=["eligibility"],
        algorithm="step_eligibility_head",
        version="1.3.0",
        role="Turns the eligibility features into one eligibility score",
    ),
    "step_multiplier": spec(
        source="step_counter_1_3_0.pt",
        core="model.step_count_multiplier",
        inputs=[("eligibility", (1, 1), F32)],
        outputs=["multiplier"],
        algorithm="step_count_multiplier",
        version="1.3.0",
        role="Parameterised sigmoid scaling the raw step count by eligibility",
        notes="Two parameters. Converted rather than ported so the whole archive is accounted for.",
    ),
    "awhr_profile_head": spec(
        source="awhr_profile_selector_1_0_1.pt",
        core="model.fc",
        inputs=[("features", (60, 32), F32)],
        outputs=["profile_logits"],
        algorithm="awake_heart_rate_profile_head",
        version="1.0.1",
        role="Three-way profile head over the recurrent layer's output",
    ),
    "popsicle_min_follicular": spec(
        source="popsicle_1_8_1.pt",
        core="_model_runner.min_follicular_runner.trained_prediction_model",
        inputs=[("features", (1, 9), F32)],
        outputs=["min_follicular_days"],
        algorithm="popsicle_min_follicular",
        version="1.8.1",
        role="Predicts the shortest plausible follicular phase for this wearer",
        notes="A three-layer perceptron; the archive's only head with no recurrent layer in it.",
    ),
    "cva_probes_male": spec(
        source="cva_2_1_0.pt",
        core="cva_pd.probes",
        inputs=[
            ("embeddings", (1, 128), F32),
            ("age", (1,), F32),
            ("weight", (1,), F32),
            ("bmi", (1,), F32),
        ],
        outputs=["cva", "pwv", "systolic", "diastolic"],
        arg_template=["@embeddings", "male", "@age", "@weight", "@bmi"],
        algorithm="cva_probes",
        version="2.1.0",
        role="Cardiovascular age, pulse-wave velocity and blood pressure from a CVA embedding",
        notes=(
            "The head selects its weights by a `str` gender argument sitting between tensor "
            "arguments, so the two branches ship as two artefacts and Rust picks one. This "
            "and cva_encoder together are the whole of cva_2_1_0."
        ),
    ),
    "cva_probes_female": spec(
        source="cva_2_1_0.pt",
        core="cva_pd.probes",
        inputs=[
            ("embeddings", (1, 128), F32),
            ("age", (1,), F32),
            ("weight", (1,), F32),
            ("bmi", (1,), F32),
        ],
        outputs=["cva", "pwv", "systolic", "diastolic"],
        arg_template=["@embeddings", "female", "@age", "@weight", "@bmi"],
        algorithm="cva_probes",
        version="2.1.0",
        role="The female branch of the CVA probe head",
    ),

    "activity_ensemble": spec(
        source="automatic_activity_detection_3_1_11.pt",
        core="predictor.ensemble",
        inputs=[
            ("primary", (1, 16, 260), F32),
            ("secondary", (1, 16, 260), F32),
        ],
        outputs=["segment_output"],
        algorithm="activity_ensemble",
        version="3.1.11",
        role="Combines the primary and secondary segment heads into the final per-segment output",
        notes=(
            "536,900 parameters, and the last piece of the 3.1.11 predictor: with this, all "
            "3,451,144 of its parameters ship as five cores."
        ),
    ),
    "popsicle_min_follicular_v16": spec(
        source="popsicle_1_6_0.pt",
        core="_model_runner.min_follicular_runner.trained_prediction_model",
        inputs=[("features", (1, 9), F32)],
        outputs=["min_follicular_days"],
        algorithm="popsicle_min_follicular",
        version="1.6.0",
        role="The previous generation of the follicular-length head",
    ),

    # Rebuilt cores. A scripted `nn.LSTM` has no callable forward where it sits, so an
    # equivalent layer is constructed and the archive's weights loaded into it under
    # `strict=True`. See rebuilt_cores.py.

    "awhr_profile_recurrent": spec(
        source="awhr_profile_selector_1_0_1.pt",
        core="model.rnn",
        rebuild="lstm",
        rebuild_config={"input_size": 6, "hidden_size": 16, "num_layers": 1, "bidirectional": True},
        inputs=[("features", (1, 60, 6), F32)],
        outputs=["encoded"],
        algorithm="awake_heart_rate_profile_recurrent",
        version="1.0.1",
        role="Bidirectional recurrent layer between the profile features and the profile head",
        notes="The last 3,072 parameters of the archive; with this it ships whole.",
    ),
    "popsicle_ovulation_detection": spec(
        source="popsicle_1_8_1.pt",
        core="_model_runner.ovulation_detection_runner.trained_detection_model",
        rebuild="popsicle_runner",
        rebuild_config={"input_size": 3, "hidden_size": 64, "num_layers": 3, "squash": True},
        inputs=[
            ("time_series", (1, 40, 3), F32),
            ("scalars", (1, 40, 4), F32),
        ],
        outputs=["ovulation_probability"],
        algorithm="popsicle_ovulation_detection",
        version="1.8.1",
        role="Ovulation detection over up to forty cycle days of temperature, heart rate and breath rate",
        notes=(
            "84,385 parameters: a three-layer recurrent encoder over the daily series, a four-feature scalar branch, and the layer that joins them. Rebuilt whole rather than converted in place because the scripted recurrent layer has no callable forward; see rebuilt_cores."
        ),
    ),
    "popsicle_ovulation_detection_v16": spec(
        source="popsicle_1_6_0.pt",
        core="_model_runner.ovulation_detection_runner.trained_detection_model",
        rebuild="popsicle_runner",
        rebuild_config={"input_size": 3, "hidden_size": 64, "num_layers": 3, "squash": True},
        inputs=[
            ("time_series", (1, 40, 3), F32),
            ("scalars", (1, 40, 4), F32),
        ],
        outputs=["ovulation_probability"],
        algorithm="popsicle_ovulation_detection",
        version="1.6.0",
        role="The previous generation of the ovulation detector",
    ),
    "popsicle_ovulation_prediction": spec(
        source="popsicle_1_8_1.pt",
        core="_model_runner.ovulation_prediction_runner.trained_prediction_model",
        rebuild="popsicle_runner",
        rebuild_config={"input_size": 3, "hidden_size": 64, "num_layers": 3, "squash": False},
        inputs=[
            ("time_series", (1, 40, 3), F32),
            ("scalars", (1, 40, 4), F32),
        ],
        outputs=["days"],
        algorithm="popsicle_ovulation_prediction",
        version="1.8.1",
        role="Days until the next ovulation, per cycle day",
    ),
    "popsicle_ovulation_prediction_v16": spec(
        source="popsicle_1_6_0.pt",
        core="_model_runner.ovulation_prediction_runner.trained_prediction_model",
        rebuild="popsicle_runner",
        rebuild_config={"input_size": 3, "hidden_size": 64, "num_layers": 3, "squash": False},
        inputs=[
            ("time_series", (1, 40, 3), F32),
            ("scalars", (1, 40, 4), F32),
        ],
        outputs=["days"],
        algorithm="popsicle_ovulation_prediction",
        version="1.6.0",
        role="The previous generation of the ovulation predictor",
    ),
    "popsicle_period_prediction": spec(
        source="popsicle_1_8_1.pt",
        core="_model_runner.period_prediction_runner.trained_prediction_model",
        rebuild="popsicle_runner",
        rebuild_config={"input_size": 3, "hidden_size": 64, "num_layers": 3, "squash": False},
        inputs=[
            ("time_series", (1, 40, 3), F32),
            ("scalars", (1, 40, 4), F32),
        ],
        outputs=["days"],
        algorithm="popsicle_period_prediction",
        version="1.8.1",
        role="Days until the next period, per cycle day",
    ),
    "popsicle_period_prediction_v16": spec(
        source="popsicle_1_6_0.pt",
        core="_model_runner.period_prediction_runner.trained_prediction_model",
        rebuild="popsicle_runner",
        rebuild_config={"input_size": 3, "hidden_size": 64, "num_layers": 3, "squash": False},
        inputs=[
            ("time_series", (1, 40, 3), F32),
            ("scalars", (1, 40, 4), F32),
        ],
        outputs=["days"],
        algorithm="popsicle_period_prediction",
        version="1.6.0",
        role="The previous generation of the period predictor",
    ),
    "cva_predictor_v1_base": spec(
        source="cva_1_3_0.pt",
        core="cva_pd.base_model",
        inputs=[
            ("pulse_triple", (1, 3, 256), F32),
            ("exogenous", (1, 8), F32),
        ],
        outputs=["cva", "pwv", "features"],
        algorithm="cva_predictor_v1_base",
        version="1.3.0",
        role="Previous-generation CNN cardiovascular-age network over pulse, VPG and APG",
        notes=(
            "All 317,522 parameters of the archive. The wrapper around it windows a longer "
            "pulse train into 272-column blocks and could not be traced; the network takes one "
            "block's worth — 256 samples across the pulse/VPG/APG triple — plus eight "
            "exogenous values, and the windowing is Rust's."
        ),
    ),
}
