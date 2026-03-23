export const parseJson = <T,>(text: string, fallback: T): T => {
    if (!text.trim()) return fallback;
    return JSON.parse(text) as T;
};

export const pretty = (value: unknown) => JSON.stringify(value, null, 2);

export const matchesSearch = (item: any, searchText: string) => {
    if (!searchText.trim()) return true;
    return JSON.stringify(item).toLowerCase().includes(searchText.trim().toLowerCase());
};

export const toSingularTitle = (title: string) => title.endsWith('s') ? title.slice(0, -1) : title;

export const prettyJsonValue = (value: unknown, fallback: unknown = {}) => {
    if (typeof value === 'string') {
        try {
            return pretty(JSON.parse(value));
        } catch {
            return value;
        }
    }
    return pretty(value ?? fallback);
};
