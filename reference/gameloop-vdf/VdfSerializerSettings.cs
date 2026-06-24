namespace Gameloop.Vdf;

public sealed class VdfSerializerSettings
{
    public static VdfSerializerSettings Default => new VdfSerializerSettings();

    public static VdfSerializerSettings Common => new VdfSerializerSettings
    {
        UsesEscapeSequences = true,
        UsesConditionals = false
    };

    /// <summary>
    /// Determines whether the parser should translate escape sequences (/n, /t, etc.).
    /// </summary>
    public bool UsesEscapeSequences = false;

    /// <summary>
    /// Determines whether the parser should evaluate conditional blocks ([$WINDOWS], etc.).
    /// </summary>
    public bool UsesConditionals = true;

    /// <summary>
    /// If <see cref="EvaluateConditionals"/> is set to true, only VDF properties 1) without any specified conditional logic or 2) conditional logic matching defined conditionals will be returned.
    /// </summary>
    public IReadOnlyList<string>? DefinedConditionals { get; set; }
}
