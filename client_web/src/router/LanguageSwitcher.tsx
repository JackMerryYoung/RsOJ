import { useTranslation } from "react-i18next";

import { Dropdown, Label, Option } from "@fluentui/react-components";
import { TranslateFilled } from "@fluentui/react-icons";

const LANGUAGE_OPTIONS = [
    { value: "en", labelKey: "language.en" },
    { value: "zh-CN", labelKey: "language.zhCN" },
] as const;

export default function LanguageSwitcher({ style, className }: { style?: React.CSSProperties; className?: string }) {
    const { t, i18n } = useTranslation("common");

    const currentOption = LANGUAGE_OPTIONS.find((option) => option.value === i18n.resolvedLanguage) ?? LANGUAGE_OPTIONS[0];

    return (
        <>
            <TranslateFilled fontSize={24} style={{ marginRight: "12px" }} />
            <Label style={{ marginRight: "16px" }}>{t("language.label")}</Label>
            <Dropdown
                className={className}
                style={{ minWidth: "120px", ...style }}
                value={t(currentOption.labelKey)}
                selectedOptions={[currentOption.value]}
                onOptionSelect={(_event, data) => {
                    if (data.optionValue) i18n.changeLanguage(data.optionValue);
                }}
            >
                {LANGUAGE_OPTIONS.map((option) => (
                    <Option key={option.value} value={option.value}>
                        {t(option.labelKey)}
                    </Option>
                ))}
            </Dropdown>
        </>

    );
}
